//! Unit tests for message serialization and topic management.

use crate::message::{
    message_id_fn, ContentRef, TopicManager, WrfcoinMessage, BLOCKS_TOPIC, TRANSACTIONS_TOPIC,
    WEATHER_DATA_TOPIC,
};
use crate::da_challenge::{
    compute_proof, DataAvailabilityChallenge, DataAvailabilityChallengeResponse,
};
use libp2p::gossipsub;

/// Every [`WrfcoinMessage`] variant must survive a bincode round-trip.
#[test]
fn new_block_roundtrip() {
    let original = WrfcoinMessage::NewBlock {
        block_bytes: vec![0xDE, 0xAD, 0xBE, 0xEF],
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

#[test]
fn new_transactions_roundtrip() {
    let original = WrfcoinMessage::NewTransactions {
        tx_bytes: vec![vec![1, 2, 3], vec![4, 5, 6], vec![]],
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

#[test]
fn weather_data_roundtrip() {
    let original = WrfcoinMessage::WeatherData {
        station_id: "station-42".to_string(),
        data: vec![10, 20, 30, 40, 50],
        timestamp: 1_700_000_000,
        nonce: Some("00112233445566778899aabbccddeeff".to_string()),
        station_signature: "aabbccdd".to_string(),
        station_pubkey: "eeff0011".to_string(),
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

#[test]
fn chain_status_roundtrip() {
    let original = WrfcoinMessage::ChainStatus {
        height: 999_999,
        tip_hash: [0xAB; 32],
        state_root: [0xCD; 32],
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

#[test]
fn da_challenge_roundtrip() {
    let original = WrfcoinMessage::DaChallenge {
        challenge: DataAvailabilityChallenge {
            challenge_id:
                "9d41ee4fdfb63c18c8f4a5f45dbd4177bf772ab8f1d3d3ab1a8cd7cce0beef00"
                    .to_string(),
            cid: "ipfs:bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi"
                .to_string(),
            nonce: [0x5A; 32],
            offset: 4_096,
            length: 2_048,
            issued_at: 1_700_000_123,
        },
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

#[test]
fn da_challenge_response_roundtrip() {
    let challenge = DataAvailabilityChallenge {
        challenge_id: "response-roundtrip-challenge".to_string(),
        cid: "ipfs:QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG".to_string(),
        nonce: [0xA5; 32],
        offset: 256,
        length: 16,
        issued_at: 1_700_000_456,
    };
    let fragment = b"wrfcoin-da-proof".to_vec();
    let original = WrfcoinMessage::DaChallengeResponse {
        response: DataAvailabilityChallengeResponse {
            challenge_id: challenge.challenge_id.clone(),
            data_fragment: fragment.clone(),
            proof: compute_proof(&challenge.nonce, &fragment),
            keeper_id: "keeper-17".to_string(),
            responded_at: 1_700_000_489,
        },
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

#[test]
fn forecast_task_assignment_roundtrip() {
    let original = WrfcoinMessage::ForecastTaskAssignment {
        task_id: "forecast-task-42".to_string(),
        node_id: "compute-node-west-2".to_string(),
        manifest_bytes: vec![0xFA, 0xCE, 0x00, 0x01, 0x02, 0x03],
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

#[test]
fn compute_node_heartbeat_roundtrip() {
    let original = WrfcoinMessage::ComputeNodeHeartbeat {
        node_id: "compute-node-west-2".to_string(),
        capabilities: vec![
            "wrf".to_string(),
            "gfs".to_string(),
            "satellite-assimilation".to_string(),
        ],
        region: "us-west-2".to_string(),
        active_tasks: 3,
        max_concurrent_tasks: 12,
        observation_density: Some(0.72),
        station_count: Some(64),
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

/// Empty block bytes is a valid edge case.
#[test]
fn empty_block_bytes_roundtrip() {
    let original = WrfcoinMessage::NewBlock {
        block_bytes: vec![],
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

// ---------------------------------------------------------------------------
// from_bytes deserialization size limit (MAX_MESSAGE_BYTES = 1 MiB)
// ---------------------------------------------------------------------------

/// `from_bytes` must reject a payload that exceeds the 1 MiB hard cap.
#[test]
fn from_bytes_rejects_oversized_message() {
    // Construct a NewBlock with > 1 MiB of block data. `to_bytes` has no limit,
    // so serialization succeeds; deserialization via `from_bytes` must refuse it.
    let oversized = WrfcoinMessage::NewBlock {
        block_bytes: vec![0xAA; 1_100_000], // 1.1 MiB — safely above the 1 MiB limit
    };
    let wire = oversized.to_bytes().expect("to_bytes must not fail");
    assert!(
        wire.len() > 1_048_576,
        "serialized form must exceed 1 MiB for this test to be meaningful"
    );
    assert!(
        WrfcoinMessage::from_bytes(&wire).is_err(),
        "from_bytes must return Err for payloads > 1 MiB"
    );
}

/// `from_bytes` must accept a well-formed message well under the size cap.
#[test]
fn from_bytes_accepts_message_under_limit() {
    let msg = WrfcoinMessage::NewBlock {
        block_bytes: vec![0u8; 512], // tiny, far under 1 MiB
    };
    let wire = msg.to_bytes().expect("serialize");
    assert!(
        WrfcoinMessage::from_bytes(&wire).is_ok(),
        "from_bytes must accept messages well under 1 MiB"
    );
}

// ---------------------------------------------------------------------------
// validate_inline_size — opaque-blob variants (NewBlock / NewTransactions /
//                         FinalityVote / FinalityCertificate)
// ---------------------------------------------------------------------------

/// A NewBlock whose block_bytes exceed `inline_data_max` must be rejected.
#[test]
fn new_block_over_inline_threshold_rejected() {
    let msg = WrfcoinMessage::NewBlock {
        block_bytes: vec![0u8; 100_000],
    };
    assert!(msg.validate_inline_size(65_536).is_err());
}

/// A NewBlock whose block_bytes are within `inline_data_max` must be accepted.
#[test]
fn new_block_under_inline_threshold_passes() {
    let msg = WrfcoinMessage::NewBlock {
        block_bytes: vec![0u8; 1_024],
    };
    assert!(msg.validate_inline_size(65_536).is_ok());
}

/// NewTransactions is rejected when the *total* bytes across all transactions
/// exceed `inline_data_max`.
#[test]
fn new_transactions_total_over_threshold_rejected() {
    // Three transactions, each 30 KiB — total 90 KiB, above 65_536.
    let tx = vec![0u8; 30_720];
    let msg = WrfcoinMessage::NewTransactions {
        tx_bytes: vec![tx.clone(), tx.clone(), tx],
    };
    assert!(msg.validate_inline_size(65_536).is_err());
}

/// NewTransactions is accepted when the total bytes are within `inline_data_max`.
#[test]
fn new_transactions_total_under_threshold_passes() {
    let msg = WrfcoinMessage::NewTransactions {
        tx_bytes: vec![vec![1, 2, 3], vec![4, 5, 6]],
    };
    assert!(msg.validate_inline_size(65_536).is_ok());
}

/// FinalityVote with oversized vote_bytes must be rejected.
#[test]
fn finality_vote_over_threshold_rejected() {
    let msg = WrfcoinMessage::FinalityVote {
        vote_bytes: vec![0u8; 100_000],
    };
    assert!(msg.validate_inline_size(65_536).is_err());
}

/// FinalityVote with small vote_bytes must be accepted.
#[test]
fn finality_vote_under_threshold_passes() {
    let msg = WrfcoinMessage::FinalityVote {
        vote_bytes: vec![0u8; 512],
    };
    assert!(msg.validate_inline_size(65_536).is_ok());
}

/// FinalityCertificate with oversized cert_bytes must be rejected.
#[test]
fn finality_certificate_over_threshold_rejected() {
    let msg = WrfcoinMessage::FinalityCertificate {
        cert_bytes: vec![0u8; 100_000],
    };
    assert!(msg.validate_inline_size(65_536).is_err());
}

/// FinalityCertificate with small cert_bytes must be accepted.
#[test]
fn finality_certificate_under_threshold_passes() {
    let msg = WrfcoinMessage::FinalityCertificate {
        cert_bytes: vec![0u8; 512],
    };
    assert!(msg.validate_inline_size(65_536).is_ok());
}

/// Messages with no variable-length blob fields (ChainStatus, GenesisHandshake,
/// ComputeNodeHeartbeat) must always pass `validate_inline_size` regardless of
/// the threshold, since they contain no unbounded byte vectors.
#[test]
fn fixed_schema_messages_always_pass_inline_size_check() {
    let chain_status = WrfcoinMessage::ChainStatus {
        height: 1_000_000,
        tip_hash: [0xAB; 32],
        state_root: [0xCD; 32],
    };
    assert!(chain_status.validate_inline_size(1).is_ok());

    let genesis = WrfcoinMessage::GenesisHandshake {
        network_id: "testnet".into(),
        chain_id: "wrfcoin-test".into(),
        genesis_hash: "a".repeat(64),
        protocol_version: 1,
    };
    assert!(genesis.validate_inline_size(1).is_ok());
}

/// `message_id_fn` must produce consistent hashes for the same payload.
#[test]
fn message_id_fn_is_deterministic() {
    let data = b"hello, wrfcoin gossipsub".to_vec();
    let msg = gossipsub::Message {
        source: None,
        data: data.clone(),
        sequence_number: None,
        topic: TopicManager::new().blocks_hash(),
    };

    let id1 = message_id_fn(&msg);
    let id2 = message_id_fn(&msg);
    assert_eq!(id1, id2, "same payload must produce same message id");
}

/// Different payloads must produce different message IDs (with very high
/// probability).
#[test]
fn message_id_fn_different_payloads() {
    let tm = TopicManager::new();
    let msg_a = gossipsub::Message {
        source: None,
        data: b"payload-a".to_vec(),
        sequence_number: None,
        topic: tm.blocks_hash(),
    };
    let msg_b = gossipsub::Message {
        source: None,
        data: b"payload-b".to_vec(),
        sequence_number: None,
        topic: tm.blocks_hash(),
    };
    assert_ne!(message_id_fn(&msg_a), message_id_fn(&msg_b));
}

/// [`TopicManager`] must map each topic hash back to its string name.
#[test]
fn topic_manager_hash_to_name() {
    let tm = TopicManager::new();

    assert_eq!(tm.topic_name(&tm.blocks_hash()), Some(BLOCKS_TOPIC));
    assert_eq!(
        tm.topic_name(&tm.transactions_hash()),
        Some(TRANSACTIONS_TOPIC)
    );
    assert_eq!(
        tm.topic_name(&tm.weather_data_hash()),
        Some(WEATHER_DATA_TOPIC)
    );
}

/// The three canonical topics must all be different.
#[test]
fn topic_hashes_are_distinct() {
    let tm = TopicManager::new();
    let hashes = [
        tm.blocks_hash(),
        tm.transactions_hash(),
        tm.weather_data_hash(),
    ];
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            assert_ne!(hashes[i], hashes[j], "topic hashes must be distinct");
        }
    }
}

/// A message routes to the correct topic.
#[test]
fn message_routes_to_correct_topic() {
    let tm = TopicManager::new();

    let block_msg = WrfcoinMessage::NewBlock {
        block_bytes: vec![1],
    };
    assert_eq!(block_msg.topic(&tm), tm.blocks_hash());

    let tx_msg = WrfcoinMessage::NewTransactions {
        tx_bytes: vec![vec![2]],
    };
    assert_eq!(tx_msg.topic(&tm), tm.transactions_hash());

    let weather_msg = WrfcoinMessage::WeatherData {
        station_id: "s1".into(),
        data: vec![3],
        timestamp: 0,
        nonce: Some("00112233445566778899aabbccddeeff".to_string()),
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert_eq!(weather_msg.topic(&tm), tm.weather_data_hash());

    let status_msg = WrfcoinMessage::ChainStatus {
        height: 1,
        tip_hash: [0; 32],
        state_root: [0; 32],
    };
    // ChainStatus routes to blocks topic.
    assert_eq!(status_msg.topic(&tm), tm.blocks_hash());
}

// ---------------------------------------------------------------------------
// ContentRef validation
// ---------------------------------------------------------------------------

fn valid_content_ref() -> ContentRef {
    ContentRef {
        data_cid: "QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG".to_string(),
        data_hash: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
            .to_string(),
        storage_provider: "ipfs".to_string(),
        data_size: 52_428_800,
    }
}

#[test]
fn content_ref_valid() {
    assert!(valid_content_ref().validate().is_ok());
}

#[test]
fn content_ref_empty_cid_rejected() {
    let mut cr = valid_content_ref();
    cr.data_cid = String::new();
    assert!(cr.validate().is_err());
}

#[test]
fn content_ref_bad_hash_rejected() {
    let mut cr = valid_content_ref();
    cr.data_hash = "tooshort".to_string();
    assert!(cr.validate().is_err());
}

#[test]
fn content_ref_invalid_provider_rejected() {
    let mut cr = valid_content_ref();
    cr.storage_provider = "s3".to_string();
    assert!(cr.validate().is_err());
}

// ---------------------------------------------------------------------------
// Inline size validation
// ---------------------------------------------------------------------------

#[test]
fn weather_data_under_threshold_passes() {
    let msg = WrfcoinMessage::WeatherData {
        station_id: "s1".into(),
        data: vec![0u8; 1024],
        timestamp: 100,
        nonce: Some("00112233445566778899aabbccddeeff".to_string()),
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert!(msg.validate_inline_size(65_536).is_ok());
}

#[test]
fn weather_data_over_threshold_rejected() {
    let msg = WrfcoinMessage::WeatherData {
        station_id: "s1".into(),
        data: vec![0u8; 100_000],
        timestamp: 100,
        nonce: Some("00112233445566778899aabbccddeeff".to_string()),
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert!(msg.validate_inline_size(65_536).is_err());
}

#[test]
fn weather_data_v2_with_content_ref_passes() {
    let msg = WrfcoinMessage::WeatherDataV2 {
        station_id: "s1".into(),
        data: vec![],
        timestamp: 100,
        content_ref: Some(valid_content_ref()),
        nonce: "00112233445566778899aabbccddeeff".into(), // pragma: allowlist secret
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert!(msg.validate_inline_size(65_536).is_ok());
}

#[test]
fn weather_data_v2_inline_under_threshold_passes() {
    let msg = WrfcoinMessage::WeatherDataV2 {
        station_id: "s1".into(),
        data: vec![0u8; 1024],
        timestamp: 100,
        content_ref: None,
        nonce: "00112233445566778899aabbccddeeff".into(), // pragma: allowlist secret
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert!(msg.validate_inline_size(65_536).is_ok());
}

#[test]
fn weather_data_v2_inline_over_threshold_without_ref_rejected() {
    let msg = WrfcoinMessage::WeatherDataV2 {
        station_id: "s1".into(),
        data: vec![0u8; 100_000],
        timestamp: 100,
        content_ref: None,
        nonce: "00112233445566778899aabbccddeeff".into(), // pragma: allowlist secret
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert!(msg.validate_inline_size(65_536).is_err());
}

#[test]
fn forecast_task_result_over_threshold_rejected() {
    let msg = WrfcoinMessage::ForecastTaskResult {
        task_id: "t1".into(),
        node_id: "n1".into(),
        receipt_bytes: vec![0u8; 100_000],
    };
    assert!(msg.validate_inline_size(65_536).is_err());
}

#[test]
fn forecast_task_result_v2_with_ref_passes() {
    let msg = WrfcoinMessage::ForecastTaskResultV2 {
        task_id: "t1".into(),
        node_id: "n1".into(),
        receipt_bytes: vec![],
        content_ref: Some(valid_content_ref()),
    };
    assert!(msg.validate_inline_size(65_536).is_ok());
}

/// `NewBlock` is now bounded by `inline_data_max` in `validate_inline_size`.
/// A block whose bytes exceed the threshold must be rejected, just like WeatherData.
#[test]
fn new_block_large_payload_rejected_by_inline_check() {
    let msg = WrfcoinMessage::NewBlock {
        block_bytes: vec![0u8; 500_000],
    };
    // With inline_data_max = 65_536, a 500 KiB block is too large.
    assert!(msg.validate_inline_size(65_536).is_err());
    // But it passes when the threshold is generous enough.
    assert!(msg.validate_inline_size(600_000).is_ok());
}

// ---------------------------------------------------------------------------
// Round-trip serialization for V2 variants
// ---------------------------------------------------------------------------

#[test]
fn weather_data_v2_inline_roundtrip() {
    let original = WrfcoinMessage::WeatherDataV2 {
        station_id: "station-42".into(),
        data: vec![10, 20, 30],
        timestamp: 1_700_000_000,
        content_ref: None,
        nonce: "00112233445566778899aabbccddeeff".into(), // pragma: allowlist secret
        station_signature: "deadbeef".into(),
        station_pubkey: "cafebabe".into(),
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

#[test]
fn weather_data_v2_with_ref_roundtrip() {
    let original = WrfcoinMessage::WeatherDataV2 {
        station_id: "station-42".into(),
        data: vec![],
        timestamp: 1_700_000_000,
        content_ref: Some(valid_content_ref()),
        nonce: "00112233445566778899aabbccddeeff".into(), // pragma: allowlist secret
        station_signature: "deadbeef".into(),
        station_pubkey: "cafebabe".into(),
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

#[test]
fn forecast_task_result_v2_roundtrip() {
    let original = WrfcoinMessage::ForecastTaskResultV2 {
        task_id: "task-001".into(),
        node_id: "node-abc".into(),
        receipt_bytes: vec![],
        content_ref: Some(ContentRef {
            data_cid: "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi".to_string(),
            data_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                .to_string(),
            storage_provider: "filecoin".to_string(),
            data_size: 50_000_000,
        }),
    };
    let bytes = original.to_bytes().expect("serialize");
    let decoded = WrfcoinMessage::from_bytes(&bytes).expect("deserialize");
    assert_eq!(original, decoded);
}

// ---------------------------------------------------------------------------
// Topic routing for V2 variants
// ---------------------------------------------------------------------------

#[test]
fn weather_data_v2_routes_to_weather_topic() {
    let tm = TopicManager::new();
    let msg = WrfcoinMessage::WeatherDataV2 {
        station_id: "s1".into(),
        data: vec![],
        timestamp: 0,
        content_ref: None,
        nonce: "00112233445566778899aabbccddeeff".into(), // pragma: allowlist secret
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert_eq!(msg.topic(&tm), tm.weather_data_hash());
}

#[test]
fn forecast_task_result_v2_routes_to_forecast_topic() {
    let tm = TopicManager::new();
    let msg = WrfcoinMessage::ForecastTaskResultV2 {
        task_id: "t1".into(),
        node_id: "n1".into(),
        receipt_bytes: vec![],
        content_ref: None,
    };
    assert_eq!(msg.topic(&tm), tm.forecast_tasks_hash());
}

// ---------------------------------------------------------------------------
// Malformed-input rejection (#1735 hardening suite)
//
// `from_bytes` must never panic on adversarial input — it must return `Err`.
// These tests guard against bincode panicking on zero-length or random bytes.
// ---------------------------------------------------------------------------

/// Zero-length input must return `Err`, not panic.
///
/// An empty slice is well under the 1 MiB size cap, so the early-exit guard
/// does not fire — bincode sees an empty slice and must report a decode error.
#[test]
fn from_bytes_rejects_empty_input() {
    let result = WrfcoinMessage::from_bytes(&[]);
    assert!(
        result.is_err(),
        "from_bytes must return Err on zero-length input, not panic"
    );
}

/// A single zero byte must return `Err` (no valid message fits in 1 byte).
#[test]
fn from_bytes_rejects_single_byte_input() {
    let result = WrfcoinMessage::from_bytes(&[0x00]);
    assert!(result.is_err(), "from_bytes must return Err on 1-byte input");
}

/// Bytes that encode an out-of-range enum variant index must return `Err`.
///
/// WrfcoinMessage has 15 variants (indices 0–14). A 4-byte LE variant index
/// of 255 is not a valid discriminant and must be rejected by bincode.
#[test]
fn from_bytes_rejects_invalid_variant_index() {
    // Variant index 255 (LE) followed by enough zero padding so bincode
    // doesn't reject for a length reason before checking the discriminant.
    let mut buf = vec![0u8; 64];
    buf[0] = 255; // LE u32 = 255, a non-existent variant
    let result = WrfcoinMessage::from_bytes(&buf);
    assert!(
        result.is_err(),
        "from_bytes must return Err for an invalid enum variant index (255)"
    );
}

/// A buffer of high-entropy pseudo-random bytes (seeded deterministically so
/// this test is stable across runs) must return `Err` — random noise is not a
/// valid bincode message.
#[test]
fn from_bytes_rejects_random_noise() {
    // LCG with seed 0xdeadbeef — deterministic, no external dependency.
    let mut state: u64 = 0xdeadbeef_cafef00d;
    let noise: Vec<u8> = (0..256)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 56) as u8
        })
        .collect();

    let result = WrfcoinMessage::from_bytes(&noise);
    assert!(
        result.is_err(),
        "from_bytes must return Err on 256 bytes of pseudo-random noise"
    );
}

/// One byte over the 1 MiB hard cap (1_048_577 bytes) must be rejected by the
/// size guard, regardless of what the bytes contain.
///
/// This pins the exact boundary of `MAX_MESSAGE_BYTES = 1_048_576`:
///   • ≤ 1_048_576 bytes: size guard passes (bincode decides validity)
///   • > 1_048_576 bytes: size guard fires immediately with a Custom error
#[test]
fn from_bytes_rejects_one_byte_over_limit() {
    let over = vec![0u8; 1_048_577]; // MAX_MESSAGE_BYTES + 1
    let result = WrfcoinMessage::from_bytes(&over);
    assert!(
        result.is_err(),
        "from_bytes must return Err for payloads 1 byte over the 1 MiB hard cap"
    );
}

// ---------------------------------------------------------------------------
// Additional gossip decode hardening (#1735 — slice 3c)
// ---------------------------------------------------------------------------

/// A valid message that has been truncated mid-stream must return `Err`.
///
/// Truncated frames are the most common real-world malformed input on the
/// gossip overlay (MTU fragmentation, premature connection close).  bincode
/// must report an error, not panic.
#[test]
fn from_bytes_truncated_valid_message_returns_err() {
    // Build a valid NewBlock message and cut it exactly in half.
    let valid = WrfcoinMessage::NewBlock {
        block_bytes: vec![0xAB; 64],
    };
    let wire = valid.to_bytes().expect("valid message must serialize");
    assert!(wire.len() >= 2, "serialized message must be at least 2 bytes");
    let truncated = &wire[..wire.len() / 2];
    assert!(
        WrfcoinMessage::from_bytes(truncated).is_err(),
        "from_bytes must return Err on a truncated (mid-stream) message"
    );
}

/// A trailing-garbage message: valid prefix + appended junk bytes.
///
/// bincode's default deserializer reads exactly what it needs and ignores
/// trailing bytes — so this variant SUCCEEDS.  This pins the behavior:
/// if the implementation ever switches to a strict no-trailing-bytes mode,
/// the test will alert the developer.
#[test]
fn from_bytes_with_trailing_garbage_succeeds() {
    let valid = WrfcoinMessage::NewBlock {
        block_bytes: vec![0xCD; 8],
    };
    let mut wire = valid.to_bytes().expect("serialize");
    wire.extend_from_slice(b"trailing-garbage");
    // bincode ignores trailing bytes by default; the message still decodes.
    let decoded = WrfcoinMessage::from_bytes(&wire);
    assert!(
        decoded.is_ok(),
        "from_bytes must tolerate trailing bytes (bincode ignores them): {decoded:?}"
    );
}

/// `ContentRef::validate` must reject a CID that exceeds 256 bytes.
///
/// The 256-byte cap prevents a misbehaving peer from embedding an
/// arbitrarily-long URL-like CID that balloons allocation or log output.
#[test]
fn content_ref_cid_exceeds_max_length_rejected() {
    let mut cr = ContentRef {
        data_cid: "x".repeat(257), // one byte over the cap
        data_hash: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
            .to_string(),
        storage_provider: "ipfs".to_string(),
        data_size: 1024,
    };
    let err = cr.validate().expect_err("CID longer than 256 bytes must be rejected");
    assert!(
        err.contains("exceeds maximum"),
        "error must mention the length cap: {err}"
    );

    // Exactly at the cap is fine.
    cr.data_cid = "x".repeat(256);
    assert!(
        cr.validate().is_ok(),
        "CID of exactly 256 bytes must be accepted"
    );
}

/// `ContentRef::validate` must reject a CID that begins with a space.
///
/// Surrounding whitespace is forbidden to prevent HTTP header injection
/// and log-confusion attacks (leading/trailing spaces produce invisible
/// mismatch between the stored CID and the fetch URL).
#[test]
fn content_ref_cid_leading_whitespace_rejected() {
    let cr = ContentRef {
        data_cid: " valid-cid-without-scheme".to_string(),
        data_hash: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
            .to_string(),
        storage_provider: "ipfs".to_string(),
        data_size: 512,
    };
    let err = cr.validate().expect_err("CID with leading whitespace must be rejected");
    assert!(
        err.contains("whitespace"),
        "error must mention whitespace: {err}"
    );
}

/// `validate_inline_size(0)` must reject any message that carries even a
/// single byte of inline data.
///
/// A zero threshold is the strictest configuration.  This test pins that
/// the comparison is `> inline_data_max` (strict), so data.len() == 1
/// with inline_data_max == 0 is rejected but data.len() == 0 is accepted.
#[test]
fn validate_inline_size_zero_threshold_rejects_nonempty_inline_data() {
    let msg_with_data = WrfcoinMessage::WeatherData {
        station_id: "s1".into(),
        data: vec![0u8; 1],
        timestamp: 0,
        nonce: None,
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert!(
        msg_with_data.validate_inline_size(0).is_err(),
        "1 byte of inline data must be rejected when inline_data_max = 0"
    );

    // Empty data at the zero threshold must be accepted.
    let msg_empty_data = WrfcoinMessage::WeatherData {
        station_id: "s1".into(),
        data: vec![],
        timestamp: 0,
        nonce: None,
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert!(
        msg_empty_data.validate_inline_size(0).is_ok(),
        "empty inline data must be accepted even at the zero threshold"
    );
}

/// A `WeatherDataV2` message with a valid `content_ref` must pass
/// `validate_inline_size` even when `data` contains bytes that exceed the
/// inline threshold.
///
/// The production logic skips the inline-size check when `content_ref` is
/// provided (the CID is the authoritative data reference; the inline field
/// should be empty but this is not enforced at the validate layer).
#[test]
fn weather_data_v2_large_inline_with_content_ref_passes_inline_check() {
    let cr = ContentRef {
        data_cid: "QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG".to_string(),
        data_hash: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
            .to_string(),
        storage_provider: "filecoin".to_string(),
        data_size: 10_000_000,
    };
    let msg = WrfcoinMessage::WeatherDataV2 {
        station_id: "s-offload".into(),
        // Large inline data — would fail if there were no content_ref.
        data: vec![0xBE; 100_000],
        timestamp: 1_700_000_000,
        content_ref: Some(cr),
        nonce: "00112233445566778899aabbccddeeff".into(), // pragma: allowlist secret
        station_signature: String::new(),
        station_pubkey: String::new(),
    };
    assert!(
        msg.validate_inline_size(65_536).is_ok(),
        "WeatherDataV2 with a valid content_ref must pass validate_inline_size \
         even when inline data exceeds the threshold"
    );
}

/// #2884: every wire identifier must carry [`WIRE_FORMAT_VERSION`].
///
/// Three payload-layout breaks shipped on unchanged identifiers in one month
/// (Block `ae0a22833`, FinalityVote `927e239ed`, BlockResponse `0bb151ff9`) —
/// survivable only because flag-day redeploys happened to follow. This test is
/// the coupling that was missing: bumping `WIRE_FORMAT_VERSION` (the
/// acknowledgement a wire layout changed) without renaming every identifier
/// fails here, and the renamed identifiers move version skew from
/// per-payload decode failures to the identifier layer, where the skew
/// detectors (`detect_block_sync_version_skew`,
/// `detect_topic_version_skew` — operator-signal tests below) can name both
/// generations. This test proves only identifier-string coupling; the
/// operator-visible attribution is proven by those tests, not this one.
///
/// If this test brought you here after a version bump: rename each string to
/// the asserted value, and choose the rollout explicitly — dual-version
/// support for one release, or a declared flag-day. See the
/// `WIRE_FORMAT_VERSION` doc for both halves.
#[test]
fn wire_identifiers_carry_the_wire_format_version() {
    use crate::message::{
        DA_CHALLENGES_TOPIC, FINALITY_VOTES_TOPIC, FORECAST_TASKS_TOPIC,
        GENESIS_HANDSHAKE_TOPIC, WIRE_FORMAT_VERSION,
    };
    use crate::sync::BLOCK_SYNC_PROTOCOL;

    let v = WIRE_FORMAT_VERSION;
    let expected_topics = [
        (BLOCKS_TOPIC, format!("wrfcoin/blocks/{v}")),
        (TRANSACTIONS_TOPIC, format!("wrfcoin/transactions/{v}")),
        (WEATHER_DATA_TOPIC, format!("wrfcoin/weather-data/{v}")),
        (FORECAST_TASKS_TOPIC, format!("wrfcoin/forecast-tasks/{v}")),
        (DA_CHALLENGES_TOPIC, format!("wrfcoin/da-challenges/{v}")),
        (
            GENESIS_HANDSHAKE_TOPIC,
            format!("wrfcoin/genesis-handshake/{v}"),
        ),
        (FINALITY_VOTES_TOPIC, format!("wrfcoin/finality-votes/{v}")),
    ];
    for (actual, expected) in expected_topics {
        assert_eq!(
            actual, expected,
            "gossip topic must carry WIRE_FORMAT_VERSION ({v}) — a layout \
             bump without an identifier rename ships undiagnosable skew (#2884)"
        );
    }
    assert_eq!(
        BLOCK_SYNC_PROTOCOL,
        format!("/wrfcoin/block-sync/{v}.0.0"),
        "the block-sync protocol id must carry WIRE_FORMAT_VERSION ({v}) — \
         renaming it is what makes skewed peers refuse at negotiation (#2884)"
    );
}

// ---------------------------------------------------------------------------
// #2884 version-skew attribution — the operator-visible signal
// ---------------------------------------------------------------------------
//
// The tripwire test above proves identifier-string coupling. These tests
// prove the claim the PR actually makes to an operator: when a peer speaks a
// different wire generation, the surfaced warning NAMES the local and remote
// identifiers and versions. The reports asserted here are exactly the
// strings the node.rs identify / gossipsub-subscription / block-sync-failure
// handlers emit.

/// A skewed peer's identify protocol list yields a report naming both
/// block-sync generations and both wire-format versions.
#[test]
fn block_sync_skew_report_names_both_protocols_and_versions() {
    use crate::message::{detect_block_sync_version_skew, WIRE_FORMAT_VERSION};
    use crate::sync::BLOCK_SYNC_PROTOCOL;

    let v = WIRE_FORMAT_VERSION;
    let remote_protocol = format!("/wrfcoin/block-sync/{}.0.0", v + 1);
    let peer = libp2p::PeerId::random();

    let skew = detect_block_sync_version_skew(
        ["/ipfs/id/1.0.0", remote_protocol.as_str(), "/meshsub/1.1.0"],
    )
    .expect("a foreign block-sync generation must be detected as skew");

    assert_eq!(skew.local_protocol, BLOCK_SYNC_PROTOCOL);
    assert_eq!(skew.remote_protocol, remote_protocol);
    assert_eq!(skew.remote_wire_version(), Some(v + 1));

    let report = skew.operator_report(&peer);
    assert!(
        report.contains(BLOCK_SYNC_PROTOCOL),
        "report must name the LOCAL protocol: {report}"
    );
    assert!(
        report.contains(&remote_protocol),
        "report must name the REMOTE protocol: {report}"
    );
    assert!(
        report.contains(&format!("wire format {v}")),
        "report must name the local wire-format version: {report}"
    );
    assert!(
        report.contains(&format!("wire format {}", v + 1)),
        "report must name the remote wire-format version: {report}"
    );
    assert!(
        report.contains(&peer.to_string()),
        "report must name the peer: {report}"
    );
    assert!(
        report.contains("UnsupportedProtocols") && report.contains("gossip"),
        "report must attribute BOTH failure surfaces (block-sync negotiation \
         and gossip topic non-intersection): {report}"
    );
}

/// No skew is reported for a peer that speaks our generation (even alongside
/// others, i.e. a dual-version peer mid-rollout) or for a non-wrfcoin peer.
#[test]
fn block_sync_skew_not_reported_for_compatible_or_foreign_peers() {
    use crate::message::{detect_block_sync_version_skew, WIRE_FORMAT_VERSION};
    use crate::sync::BLOCK_SYNC_PROTOCOL;

    let next = format!("/wrfcoin/block-sync/{}.0.0", WIRE_FORMAT_VERSION + 1);

    assert_eq!(
        detect_block_sync_version_skew([BLOCK_SYNC_PROTOCOL]),
        None,
        "a peer speaking our exact protocol is not skew"
    );
    assert_eq!(
        detect_block_sync_version_skew([next.as_str(), BLOCK_SYNC_PROTOCOL]),
        None,
        "a dual-version peer that still speaks our generation interoperates — not skew"
    );
    assert_eq!(
        detect_block_sync_version_skew(["/ipfs/id/1.0.0", "/meshsub/1.1.0"]),
        None,
        "a peer with no wrfcoin block-sync protocol is not attributable as version skew"
    );
    assert_eq!(
        detect_block_sync_version_skew([]),
        None,
        "an empty protocol list is not skew"
    );
}

/// For every wire-bound gossip topic, a subscription to a different
/// generation yields a report naming both topic strings.
#[test]
fn topic_skew_report_names_both_generations_for_every_wire_topic() {
    use crate::message::{detect_topic_version_skew, WIRE_FORMAT_VERSION, WIRE_TOPICS};

    let v = WIRE_FORMAT_VERSION;
    let peer = libp2p::PeerId::random();
    for local_topic in WIRE_TOPICS {
        let base = local_topic
            .rsplit_once('/')
            .expect("wire topics end in /<version>")
            .0;
        let remote_topic = format!("{base}/{}", v + 1);

        let skew = detect_topic_version_skew(&remote_topic).unwrap_or_else(|| {
            panic!("{remote_topic} must be detected as a foreign generation of {local_topic}")
        });
        assert_eq!(skew.local_topic, local_topic);
        assert_eq!(skew.remote_topic, remote_topic);

        let report = skew.operator_report(&peer);
        assert!(
            report.contains(local_topic),
            "report must name OUR topic generation: {report}"
        );
        assert!(
            report.contains(&remote_topic),
            "report must name the PEER's topic generation: {report}"
        );
        assert!(
            report.contains(&format!("wire format {v}")),
            "report must name the local wire-format version: {report}"
        );
        assert!(
            report.contains(&peer.to_string()),
            "report must name the peer: {report}"
        );
        assert!(
            report.contains("stays connected"),
            "report must state that gossip skew is starvation of a still-connected \
             peer, not a refusal: {report}"
        );
    }
}

/// Our own topics and unrelated topics are not reported as skew.
#[test]
fn topic_skew_not_reported_for_own_or_unrelated_topics() {
    use crate::message::{detect_topic_version_skew, WIRE_TOPICS};

    for own in WIRE_TOPICS {
        assert_eq!(
            detect_topic_version_skew(own),
            None,
            "our own topic {own} is not skew"
        );
    }
    assert_eq!(
        detect_topic_version_skew("meshsub/blocks/2"),
        None,
        "an unrelated namespace is not skew"
    );
    assert_eq!(
        detect_topic_version_skew("wrfcoin/blocksextra/1"),
        None,
        "a topic sharing only a string prefix (not a path segment) is not skew"
    );
    assert_eq!(
        detect_topic_version_skew("wrfcoin/blocks"),
        None,
        "a base name with no version segment is not skew"
    );
}

/// The block-sync `UnsupportedProtocols` attribution names the local
/// protocol — the one thing libp2p's own rendering of that error omits.
#[test]
fn negotiation_refused_report_names_the_local_protocol() {
    use crate::sync::{block_sync_negotiation_refused_report, BLOCK_SYNC_PROTOCOL};

    let peer = libp2p::PeerId::random();
    let report = block_sync_negotiation_refused_report(&peer);
    assert!(
        report.contains(BLOCK_SYNC_PROTOCOL),
        "report must name the local block-sync protocol: {report}"
    );
    assert!(
        report.contains(&peer.to_string()),
        "report must name the peer: {report}"
    );
}
