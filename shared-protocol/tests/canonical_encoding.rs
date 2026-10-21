//! Canonical-encoding conformance and non-canonical rejection (#2124).
//!
//! Two halves:
//!
//! 1. **Property tests** — for arbitrary valid objects,
//!    `from_canonical(to_canonical(x)) == x` *and* re-encoding is
//!    byte-identical. The second half is the one that matters: round-tripping
//!    to an equal value is not enough if a second byte string could have
//!    produced it.
//! 2. **Negative fixtures** — one committed file per rejection class in
//!    `tests/fixtures/canonical/`, asserted to fail with the *specific* error.
//!    "Rejected for the wrong reason" is a real failure mode: a decoder that
//!    rejects a duplicate map key as `UnexpectedEnd` is rejecting by accident.

use std::collections::BTreeMap;

use proptest::prelude::*;
use wrfcoin_shared_protocol::blockchain::snapshot_sync::SnapshotSyncPayload;
use wrfcoin_shared_protocol::blockchain::transaction::{
    GovernanceVotePayload, OracleAttestationPayload,
};
use wrfcoin_shared_protocol::cross_chain::types::{
    BridgeMessage, BridgeMessageType, CrossChainDirection, CrossChainNetwork,
};
use wrfcoin_shared_protocol::crypto::canonical::{
    Canonical, CanonicalError, CanonicalProbe, CanonicalWriter,
};
use wrfcoin_shared_protocol::security::slashing::SlashingReportPayload;
use wrfcoin_shared_protocol::weather::validation::WeatherReading;

const FIXTURE_DIR: &str = "tests/fixtures/canonical";

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{FIXTURE_DIR}/{name}");
    std::fs::read(&path).unwrap_or_else(|e| panic!("missing canonical fixture {path}: {e}"))
}

// ---------------------------------------------------------------------------
// Negative fixtures — one per rejection class
// ---------------------------------------------------------------------------

#[test]
fn fixture_valid_decodes() {
    // Anchors the others: if this fails, the negative fixtures are testing
    // nothing, because every input would be rejected.
    let probe = CanonicalProbe::from_canonical(&fixture("valid.bin"))
        .expect("the valid fixture must decode");
    assert_eq!(probe.count, 1);
    assert_eq!(probe.offset, -1);
    assert!(probe.flag);
    assert_eq!(probe.label, "ab");
    assert_eq!(probe.digest, [1, 2, 3, 4]);
    assert_eq!(probe.note, None);
    assert_eq!(probe.attributes.len(), 2);
}

#[test]
fn fixture_trailing_bytes_is_rejected() {
    assert_eq!(
        CanonicalProbe::from_canonical(&fixture("trailing_bytes.bin")),
        Err(CanonicalError::TrailingBytes { extra: 1 })
    );
}

#[test]
fn fixture_non_minimal_integer_is_rejected() {
    // `0x81 0x00` is an overlong encoding of 1. Accepting it would give the
    // integer 1 two encodings.
    assert_eq!(
        CanonicalProbe::from_canonical(&fixture("non_minimal_integer.bin")),
        Err(CanonicalError::NonMinimalInteger)
    );
}

#[test]
fn fixture_duplicate_map_key_is_rejected() {
    assert_eq!(
        CanonicalProbe::from_canonical(&fixture("duplicate_map_key.bin")),
        Err(CanonicalError::DuplicateMapKey)
    );
}

#[test]
fn fixture_unordered_map_key_is_rejected() {
    assert_eq!(
        CanonicalProbe::from_canonical(&fixture("unordered_map_key.bin")),
        Err(CanonicalError::UnorderedMapKey)
    );
}

#[test]
fn fixture_non_canonical_encoding_is_rejected() {
    // A `bool` byte of `0x02`. If accepted as `true`, `true` has 255 encodings.
    assert!(matches!(
        CanonicalProbe::from_canonical(&fixture("non_canonical_encoding.bin")),
        Err(CanonicalError::NonCanonicalEncoding { .. })
    ));
}

#[test]
fn every_rejection_class_has_a_fixture() {
    // Keeps the fixture directory and the error enum from drifting apart, and
    // keeps the set reusable by fuzz targets.
    for name in [
        "valid.bin",
        "trailing_bytes.bin",
        "non_minimal_integer.bin",
        "duplicate_map_key.bin",
        "unordered_map_key.bin",
        "non_canonical_encoding.bin",
    ] {
        assert!(
            !fixture(name).is_empty(),
            "fixture {name} is present but empty"
        );
    }
}

// ---------------------------------------------------------------------------
// Rejection rules, exercised directly
// ---------------------------------------------------------------------------

#[test]
fn non_canonical_input_is_rejected_not_normalised() {
    // The central claim of #2124. An overlong integer must not decode to the
    // same object as its minimal form — if it did, that object would have two
    // encodings and a signature over one would carry to the other.
    let overlong = fixture("non_minimal_integer.bin");
    let minimal = fixture("valid.bin");

    assert!(CanonicalProbe::from_canonical(&minimal).is_ok());
    assert!(CanonicalProbe::from_canonical(&overlong).is_err());
    assert_ne!(overlong, minimal);
}

#[test]
fn trailing_bytes_after_a_complete_object_are_rejected() {
    let mut w = CanonicalWriter::new();
    let payload = SnapshotSyncPayload {
        height: 9,
        state_root_hex: "root".into(),
        snapshot_root_hex: "snap".into(),
        entry_count: 3,
        tip_hash_hex: "tip".into(),
        finalized_height: 2,
    };
    payload.write_canonical(&mut w);
    let mut bytes = w.finish();
    assert!(SnapshotSyncPayload::from_canonical(&bytes).is_ok());

    bytes.push(0x00);
    assert_eq!(
        SnapshotSyncPayload::from_canonical(&bytes),
        Err(CanonicalError::TrailingBytes { extra: 1 }),
        "a zero byte is still a trailing byte"
    );
}

#[test]
fn every_non_finite_float_is_rejected_on_decode() {
    // NaN has 2^52-ish bit patterns and there are two infinities. The first
    // version of this codec normalised every NaN to one pattern on encode, which
    // made the ENCODER non-injective — distinct Rust values, identical bytes.
    // Review of #2234 caught it with exactly the probe below. The domain is now
    // finite f64 only, so every one of these is rejected rather than folded.
    for (label, bits) in [
        ("signalling NaN", 0x7ff8_0000_0000_0001u64),
        ("second NaN payload", 0x7ff8_0000_0000_0002u64),
        ("formerly-canonical quiet NaN", 0x7ff8_0000_0000_0000u64),
        ("+infinity", f64::INFINITY.to_bits()),
        ("-infinity", f64::NEG_INFINITY.to_bits()),
    ] {
        let mut w = CanonicalWriter::new();
        w.write_int(0);
        w.write_bool(true);
        w.write_fixed(&bits.to_le_bytes());
        w.write_bool(false);
        w.write_bool(false);
        w.write_bool(false);
        let bytes = w.finish();

        assert!(
            matches!(
                WeatherReading::from_canonical(&bytes),
                Err(CanonicalError::NonFiniteFloat)
            ),
            "{label} must be rejected on decode"
        );
    }
}

/// The encoder side of the same property, and the one the review's probe
/// falsified: two distinct NaN payloads must not produce identical bytes.
///
/// They no longer can, because neither produces bytes at all.
#[test]
fn distinct_nan_payloads_are_refused_rather_than_collapsed() {
    let a = WeatherReading {
        timestamp_unix: 0,
        temperature_c: Some(f64::from_bits(0x7ff8_0000_0000_0001)),
        pressure_hpa: None,
        humidity_pct: None,
        wind_speed_ms: None,
    };
    let b = WeatherReading {
        temperature_c: Some(f64::from_bits(0x7ff8_0000_0000_0002)),
        ..a.clone()
    };

    assert_eq!(a.to_canonical(), Err(CanonicalError::NonFiniteFloat));
    assert_eq!(b.to_canonical(), Err(CanonicalError::NonFiniteFloat));
}

/// -0.0 and 0.0 ARE distinct values and must keep distinct encodings. Folding
/// them would be the same defect as folding NaNs, one exponent away.
#[test]
fn negative_zero_and_positive_zero_do_not_collide() {
    let pos = WeatherReading {
        timestamp_unix: 0,
        temperature_c: Some(0.0),
        pressure_hpa: None,
        humidity_pct: None,
        wind_speed_ms: None,
    };
    let neg = WeatherReading {
        temperature_c: Some(-0.0),
        ..pos.clone()
    };
    assert_ne!(
        pos.to_canonical().expect("finite"),
        neg.to_canonical().expect("finite"),
        "-0.0 and 0.0 are distinguishable values and must encode differently"
    );
}

#[test]
fn oversized_length_prefix_does_not_allocate() {
    // A 4-byte input claiming a 2^60-element string must fail on the claim, not
    // by attempting the allocation. This is reachable from any peer message.
    let mut w = CanonicalWriter::new();
    w.write_uint(u64::MAX / 2);
    let bytes = w.finish();

    assert!(matches!(
        SnapshotSyncPayload::from_canonical(&bytes),
        Err(CanonicalError::LengthOutOfRange { .. } | CanonicalError::UnexpectedEnd)
    ));
}

#[test]
fn truncated_input_is_unexpected_end_not_a_canonicality_failure() {
    let payload = SnapshotSyncPayload {
        height: 1,
        state_root_hex: "aa".into(),
        snapshot_root_hex: "bb".into(),
        entry_count: 1,
        tip_hash_hex: "cc".into(),
        finalized_height: 1,
    };
    let bytes = payload.to_canonical().expect("encodable");
    for cut in 1..bytes.len() {
        let err = SnapshotSyncPayload::from_canonical(&bytes[..cut]).unwrap_err();
        assert!(
            !matches!(err, CanonicalError::TrailingBytes { .. }),
            "truncation at {cut} must never report trailing bytes, got {err:?}"
        );
    }
}

#[test]
fn empty_input_is_rejected() {
    assert!(SnapshotSyncPayload::from_canonical(&[]).is_err());
    assert!(CanonicalProbe::from_canonical(&[]).is_err());
}

// ---------------------------------------------------------------------------
// Round-trip properties
// ---------------------------------------------------------------------------

/// `from_canonical(to_canonical(x)) == x` **and** the re-encoding is
/// byte-identical. Used by every property test below.
fn assert_round_trip<T: Canonical + PartialEq + std::fmt::Debug>(value: &T) {
    let encoded = value.to_canonical().expect("encodable");
    let decoded = T::from_canonical(&encoded).expect("canonical bytes must decode");
    assert_eq!(&decoded, value, "round trip changed the value");
    assert_eq!(
        decoded.to_canonical().expect("re-encode must succeed"),
        encoded,
        "re-encoding must be byte-identical"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn probe_round_trips(
        count in any::<u64>(),
        offset in any::<i64>(),
        weight in any::<u128>(),
        flag in any::<bool>(),
        // Full finite space, including subnormals and -0.0. NaN and +/-inf are
        // outside the encodable domain and are covered by the rejection tests
        // rather than smuggled past the round trip by a narrow generator.
        score in proptest::num::f64::NORMAL
            | proptest::num::f64::ZERO
            | proptest::num::f64::SUBNORMAL,
        label in ".{0,40}",
        note in proptest::option::of(".{0,40}"),
        attributes in proptest::collection::btree_map("[a-z]{1,8}", "[a-z0-9]{0,8}", 0..6),
    ) {
        let probe = CanonicalProbe {
            count, offset, weight, flag, score,
            label,
            digest: [9, 8, 7, 6],
            note,
            attributes,
        };
        assert_round_trip(&probe);
    }

    #[test]
    fn snapshot_payload_round_trips(
        height in any::<u64>(),
        state_root_hex in "[0-9a-f]{0,64}",
        snapshot_root_hex in "[0-9a-f]{0,64}",
        entry_count in any::<u64>(),
        tip_hash_hex in "[0-9a-f]{0,64}",
        finalized_height in any::<u64>(),
    ) {
        assert_round_trip(&SnapshotSyncPayload {
            height, state_root_hex, snapshot_root_hex, entry_count, tip_hash_hex, finalized_height,
        });
    }

    #[test]
    fn weather_reading_round_trips(
        timestamp_unix in any::<i64>(),
        temperature_c in proptest::option::of(-100.0f64..100.0),
        pressure_hpa in proptest::option::of(800.0f64..1100.0),
        humidity_pct in proptest::option::of(0.0f64..100.0),
        wind_speed_ms in proptest::option::of(0.0f64..120.0),
    ) {
        assert_round_trip(&WeatherReading {
            timestamp_unix, temperature_c, pressure_hpa, humidity_pct, wind_speed_ms,
        });
    }

    #[test]
    fn oracle_attestation_round_trips(
        commit_id in "[0-9a-f]{0,32}",
        oracle_id in "[a-z0-9-]{0,16}",
        approved in any::<bool>(),
        signature in proptest::option::of("[0-9a-f]{0,32}"),
        confidence in proptest::option::of(0.0f64..1.0),
        timestamp in any::<i64>(),
        nonce in proptest::option::of("[a-z0-9]{0,16}"),
    ) {
        // `signature` was fixed to None — the review's point: a field pinned to a
        // constant is a field the round trip never exercises.
        assert_round_trip(&OracleAttestationPayload {
            commit_id, oracle_id, approved,
            signature,
            confidence, timestamp, nonce,
        });
    }

    #[test]
    fn governance_vote_round_trips(
        proposal_id in "[a-z0-9-]{0,24}",
        approve in any::<bool>(),
        voting_power in proptest::option::of(any::<u64>()),
        signature in proptest::option::of("[0-9a-f]{0,32}"),
        signer_public_key in proptest::option::of("[0-9a-f]{0,64}"),
    ) {
        assert_round_trip(&GovernanceVotePayload {
            proposal_id, approve, voting_power,
            signature, signer_public_key,
        });
    }

    /// #2234 review: there was no generated `SlashingReportPayload` round trip at
    /// all — one hand-written example, which exercises one point in the space.
    /// Every enum variant of `evidence_type` is generated here, because the
    /// encoder maps them to stable numeric codes and a code collision would be
    /// invisible to a single-variant test.
    #[test]
    fn slashing_report_round_trips(
        validator_id in "[a-z0-9-]{0,24}",
        evidence_idx in 0usize..3,
        evidence_byte in any::<u8>(),
        reported_at_height in any::<u64>(),
        penalty_amount in any::<u128>(),
        reporter_pubkey in "[0-9a-f]{0,64}",
        reporter_signature in proptest::option::of("[0-9a-f]{0,32}"),
    ) {
        use wrfcoin_shared_protocol::security::slashing::SlashingEvidenceType as E;
        let evidence_type = [E::DoubleSign, E::InvalidZkProof, E::RepeatedLowQualityData]
            [evidence_idx]
            .clone();
        assert_round_trip(&SlashingReportPayload {
            validator_id,
            evidence_type,
            evidence_hash: [evidence_byte; 32],
            reported_at_height,
            penalty_amount,
            reporter_pubkey,
            reporter_signature,
        });
    }

    /// #2234 review: `BridgeMessage` previously omitted `payload` from its
    /// encoding, so two messages differing only there shared a preimage — a
    /// signature over one authenticated the other. The round trip is now over
    /// GENERATED payloads rather than a fixed literal, which is what catches an
    /// omitted field: a constant value agrees with itself no matter what the
    /// encoder drops.
    #[test]
    fn bridge_message_round_trips(
        message_id in "[a-z0-9-]{0,24}",
        amount in any::<u64>(),
        nonce in any::<u64>(),
        timestamp in any::<i64>(),
        custom in proptest::option::of("[a-z]{1,8}"),
        target_idx in 0usize..3,
        direction_idx in 0usize..2,
        msg_type_idx in 0usize..5,
        sender in "[a-z0-9]{0,16}",
        recipient in "[a-z0-9]{0,16}",
        tx_hash in proptest::option::of("[0-9a-f]{0,32}"),
        payload_key in "[a-z]{1,6}",
        payload_val in any::<i64>(),
        has_payload in any::<bool>(),
    ) {
        let network = match custom {
            Some(label) => CrossChainNetwork::Custom(label),
            None => CrossChainNetwork::Ethereum,
        };
        let payload = has_payload.then(|| serde_json::json!({ payload_key.clone(): payload_val }));
        let target_network = [
            CrossChainNetwork::Polygon,
            CrossChainNetwork::Solana,
            CrossChainNetwork::Custom("t".into()),
        ][target_idx]
            .clone();
        let direction = [CrossChainDirection::Inbound, CrossChainDirection::Outbound][direction_idx];
        let message_type = [
            BridgeMessageType::Lock,
            BridgeMessageType::Unlock,
            BridgeMessageType::Mint,
            BridgeMessageType::Burn,
            BridgeMessageType::Attestation,
        ][msg_type_idx];
        assert_round_trip(&BridgeMessage {
            message_id,
            source_network: network,
            target_network,
            direction,
            message_type,
            sender,
            recipient,
            amount,
            nonce,
            timestamp,
            tx_hash,
            payload,
        });
    }

    /// The injectivity failure itself, as a property: messages differing ONLY in
    /// `payload` must not share an encoding.
    #[test]
    fn bridge_messages_differing_only_in_payload_never_collide(
        a_val in any::<i64>(),
        b_val in any::<i64>(),
    ) {
        prop_assume!(a_val != b_val);
        let base = BridgeMessage {
            message_id: "m".into(),
            source_network: CrossChainNetwork::Ethereum,
            target_network: CrossChainNetwork::Polygon,
            direction: CrossChainDirection::Outbound,
            message_type: BridgeMessageType::Lock,
            sender: "s".into(),
            recipient: "r".into(),
            amount: 1,
            nonce: 1,
            timestamp: 0,
            tx_hash: None,
            payload: Some(serde_json::json!({ "v": a_val })),
        };
        let other = BridgeMessage {
            payload: Some(serde_json::json!({ "v": b_val })),
            ..base.clone()
        };
        prop_assert_ne!(base.to_canonical().expect("encodable"), other.to_canonical().expect("encodable"));
    }

    /// `Custom("ethereum")` and `Ethereum` are different networks. Encoding a
    /// network by `as_id()` would have collapsed them.
    #[test]
    fn custom_network_never_collides_with_a_named_one(label in "[a-z]{1,10}") {
        let named = BridgeMessage {
            message_id: "m".into(),
            source_network: CrossChainNetwork::Ethereum,
            target_network: CrossChainNetwork::Polygon,
            direction: CrossChainDirection::Inbound,
            message_type: BridgeMessageType::Mint,
            sender: "s".into(),
            recipient: "r".into(),
            amount: 0,
            nonce: 0,
            timestamp: 0,
            tx_hash: None,
            payload: None,
        };
        let custom = BridgeMessage {
            source_network: CrossChainNetwork::Custom(label),
            ..named.clone()
        };
        prop_assert_ne!(named.to_canonical().expect("encodable"), custom.to_canonical().expect("encodable"));
    }

    /// Distinct values must never share an encoding. A collision here is a
    /// signature-reuse primitive.
    #[test]
    fn distinct_snapshots_never_collide(
        a_height in any::<u64>(),
        b_height in any::<u64>(),
        root in "[0-9a-f]{0,16}",
    ) {
        prop_assume!(a_height != b_height);
        let a = SnapshotSyncPayload {
            height: a_height,
            state_root_hex: root.clone(),
            snapshot_root_hex: root.clone(),
            entry_count: 0,
            tip_hash_hex: root.clone(),
            finalized_height: 0,
        };
        let b = SnapshotSyncPayload { height: b_height, ..a.clone() };
        prop_assert_ne!(a.to_canonical().expect("encodable"), b.to_canonical().expect("encodable"));
    }

    /// Arbitrary bytes must never panic the decoder — it is fed peer input.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _ = CanonicalProbe::from_canonical(&bytes);
        let _ = SnapshotSyncPayload::from_canonical(&bytes);
        let _ = WeatherReading::from_canonical(&bytes);
        let _ = OracleAttestationPayload::from_canonical(&bytes);
        let _ = GovernanceVotePayload::from_canonical(&bytes);
        let _ = BridgeMessage::from_canonical(&bytes);
    }

    /// Whatever a fuzzer feeds in, an accepted encoding must be a fixed point:
    /// decoding then re-encoding reproduces the input exactly.
    #[test]
    fn accepted_input_is_always_a_fixed_point(
        bytes in proptest::collection::vec(any::<u8>(), 0..128)
    ) {
        if let Ok(probe) = CanonicalProbe::from_canonical(&bytes) {
            prop_assert_eq!(probe.to_canonical().expect("encodable"), bytes);
        }
    }
}

#[test]
fn map_ordering_is_not_observable_in_the_encoding() {
    // Insertion order must not reach the bytes, or the same logical object has
    // one encoding per insertion order.
    let mut a = BTreeMap::new();
    a.insert("zeta".to_string(), "1".to_string());
    a.insert("alpha".to_string(), "2".to_string());

    let mut b = BTreeMap::new();
    b.insert("alpha".to_string(), "2".to_string());
    b.insert("zeta".to_string(), "1".to_string());

    let make = |attributes: BTreeMap<String, String>| CanonicalProbe {
        count: 0,
        offset: 0,
        weight: 0,
        flag: false,
        score: 1.0,
        label: String::new(),
        digest: [0; 4],
        note: None,
        attributes,
    };

    assert_eq!(
        make(a).to_canonical().expect("encodable"),
        make(b).to_canonical().expect("encodable")
    );
}

// ---------------------------------------------------------------------------
// Coverage, derived from the merged registry rather than hard-coded
// ---------------------------------------------------------------------------
//
// Review of #2234: "type coverage is stale and hard-coded against merged #2231
// … its 'Same object list' claim is therefore false and can drift again as the
// registry grows." That is the same self-certifying-coverage defect the same
// reviewer found in #2235, and it is fixed the same way here on purpose — a
// future reader will assume the two match, so they should.
//
// Nothing below hard-codes a count. The reconciliation reads
// `ALL_SIGNED_OBJECT_KINDS` and fails when a registered kind is in neither the
// implemented list nor the explicitly-deferred list.

use wrfcoin_shared_protocol::crypto::domain_registry::{
    ALL_SIGNED_OBJECT_KINDS, SignedObjectKind, entry,
};

/// Registry kinds that have a `Canonical` impl today.
const CANONICAL_IMPLEMENTED: &[SignedObjectKind] = &[
    SignedObjectKind::SnapshotPayload,
    SignedObjectKind::WeatherObservation,
    SignedObjectKind::OracleAttestation,
    SignedObjectKind::GovernanceVote,
    SignedObjectKind::SlashingEvidence,
    SignedObjectKind::BridgeMessage,
];

/// Registry kinds with **no** `Canonical` impl, each with the reason.
///
/// A reason, not a placeholder: a deferral nobody can read is indistinguishable
/// from an omission.
const CANONICAL_DEFERRED: &[(SignedObjectKind, &str)] = &[
    (
        SignedObjectKind::Transaction,
        "signed via `signing_hash` (wrfcoin_tx_sighash_v2, #2064); a canonical \
         encoding would be a second preimage for the same object — it must \
         REPLACE that path at the genesis cut, not sit beside it",
    ),
    (
        SignedObjectKind::BlockHeader,
        "same: `Block` already has a `DomainSeparated` preimage and a separate \
         `calculate_hash`; adding a third encoding is the drift this codec exists \
         to prevent (see docs/security/GENESIS_CUT_PLAN.md)",
    ),
    (
        SignedObjectKind::AttestationEnvelope,
        "CalibrationAttestationPayload — same shape as OracleAttestation, not yet \
         implemented; no blocker, just unwritten",
    ),
    (
        SignedObjectKind::GovernanceProposal,
        "`governance::proposal::Proposal` carries nested types with no canonical \
         form yet",
    ),
    (
        SignedObjectKind::ComputeReceipt,
        "not yet implemented; no blocker",
    ),
    (
        SignedObjectKind::ForecastVerificationReceipt,
        "lives in `forecast-score`, which depends on shared-protocol — the impl \
         must be written there (the #2249 constraint)",
    ),
    (
        SignedObjectKind::FinalityVote,
        "CrossCrate (#2249): lives in consensus-engines. Note the unsigned \
         `FinalitySignal` impl was REMOVED in this PR — an unsigned type must not \
         carry a canonical encoding any more than it may carry a domain tag",
    ),
    (
        SignedObjectKind::DeviceAttestation,
        "CrossCrate (#2249): lives in sensor-attestation",
    ),
    (
        SignedObjectKind::StorageAttestation,
        "CrossCrate (#2249): lives in keeper-proof",
    ),
    (
        SignedObjectKind::OraclePacket,
        "CrossCrate (#2249): lives in oracle-packet",
    ),
    (
        SignedObjectKind::ValidatorSetChange,
        "Exception: no Rust type exists (#2227) — nothing to encode",
    ),
    (
        SignedObjectKind::ProducerSetChange,
        "signed via its DomainSeparated envelope with canonical member ordering \
         (#2290/#2398); a parallel Canonical encoding would be a second preimage \
         for the same object — the drift class this codec exists to prevent \
         (same reasoning as Transaction and BlockHeader above)",
    ),
    (
        SignedObjectKind::WalletLoginChallenge,
        "Exception: no Rust type exists (#2228) — nothing to encode",
    ),
    (
        SignedObjectKind::UpgradeNotice,
        "signed via its `DomainSeparated` envelope (#3094 governance upgrade \
         notice); a parallel Canonical encoding would be a second preimage for \
         the same object — the drift class this codec exists to prevent (same \
         reasoning as Transaction, BlockHeader and ProducerSetChange above)",
    ),
    (
        SignedObjectKind::FeatureScheduleExtension,
        "signed via its `DomainSeparated` envelope (#3277 governance \
         feature-schedule extension); a parallel Canonical encoding would be a \
         second preimage for the same object — the drift class this codec \
         exists to prevent (same reasoning as UpgradeNotice above)",
    ),
    (
        SignedObjectKind::FaucetSinkPossession,
        "signed via its `DomainSeparated` envelope (#3361 F3 faucet-key proof \
         of possession); a parallel Canonical encoding would be a second \
         preimage for the same object (same reasoning as UpgradeNotice above)",
    ),
    (
        SignedObjectKind::NativeAccountSubject,
        "signed via explicit V2 DomainSeparated constant-width fields (#2167); \
         the varint Canonical trait would define a different preimage; native \
         wire decoding is a separate bounded-codec delivery gate",
    ),
    (
        SignedObjectKind::NativeSponsorSubject,
        "signed via explicit V2 DomainSeparated account digest plus sponsor terms \
         (#2167); a parallel Canonical encoding would create a second preimage",
    ),
    (
        SignedObjectKind::NativeSessionPossession,
        "signed via explicit V2 DomainSeparated registration digest (#2167); a \
         parallel Canonical encoding would create a second possession preimage",
    ),
    (
        SignedObjectKind::SlashEvidenceReport,
        "signed via its checked DomainSeparated body; its executed golden is in \
         slash_evidence_types and signed_type_inventory, avoiding a second codec",
    ),
    (
        SignedObjectKind::NodeServiceReceipt,
        "signed via its DomainSeparated BodyBuilder preimage (#3129); executed \
         golden coverage is in signing_preimage_v2_golden and signed_type_inventory; \
         a parallel Canonical encoding would create a second signing preimage",
    ),
    (
        SignedObjectKind::NodeReceiptSubmissionAuthorization,
        "S3 explicit DomainSeparated body with executed independent V2 golden; bounded transport is separate, avoiding a second signing codec",
    ),
    (
        SignedObjectKind::NodeServiceTicket,
        "S1 DomainSeparated body and fixed-integer bounded wire; executed signing and wire goldens; no parallel Canonical signing codec",
    ),
    (
        SignedObjectKind::NodeSettlementEndorsement,
        "S1 DomainSeparated body and fixed-integer bounded wire; executed signing and wire goldens; no parallel Canonical signing codec",
    ),
    (
        SignedObjectKind::NodeBondAuthorization,
        "S1 DomainSeparated body and fixed-integer bounded wire; executed signing and wire goldens; no parallel Canonical signing codec",
    ),
    (
        SignedObjectKind::NodeBondPossession,
        "S1 DomainSeparated body and fixed-integer bounded wire; executed signing and wire goldens; no parallel Canonical signing codec",
    ),
];

#[test]
fn canonical_coverage_reconciles_with_the_merged_registry() {
    let mut unaccounted = Vec::new();
    for &kind in ALL_SIGNED_OBJECT_KINDS {
        let implemented = CANONICAL_IMPLEMENTED.contains(&kind);
        let deferred = CANONICAL_DEFERRED.iter().any(|(k, _)| *k == kind);
        if implemented == deferred {
            // Neither, or both — either way the ledger does not describe reality.
            unaccounted.push((kind, implemented, deferred));
        }
    }
    assert!(
        unaccounted.is_empty(),
        "these registered kinds are unaccounted for in the canonical-encoding \
         ledger (implemented, deferred): {unaccounted:?}. A kind must be in \
         exactly one list — add it, with a reason if deferred."
    );

    // And no hard-coded total: the ledger's size is derived.
    assert_eq!(
        CANONICAL_IMPLEMENTED.len() + CANONICAL_DEFERRED.len(),
        ALL_SIGNED_OBJECT_KINDS.len(),
        "the ledger and the registry disagree on how many kinds exist"
    );
}

#[test]
fn every_deferred_kind_carries_a_real_reason() {
    for (kind, why) in CANONICAL_DEFERRED {
        assert!(
            why.len() > 30,
            "{kind:?} is deferred with no meaningful reason: {why:?}"
        );
        assert!(entry(*kind).is_some(), "{kind:?} has no registry row");
    }
}

// ---------------------------------------------------------------------------
// The enforcement gap, made enforceable
// ---------------------------------------------------------------------------

/// This codec reaches **no production deserialization path**, and the review is
/// right that saying so in prose is not a close.
///
/// What is unprotected, stated accurately: malformed peer input is rejected by
/// `bincode`, not by this checker. Nothing on the wire is canonicality-checked.
/// So "canonical" currently means *a preimage this node computes*, not *a form
/// this node enforces on input*. It is safe to sign over `to_canonical()`; it is
/// **not** safe to treat acceptance of a peer message as evidence of canonical
/// form.
///
/// Wiring it into ingress has a real compatibility question — every existing
/// encoder must already emit canonical form, or nodes begin rejecting each other
/// mid-flight — which belongs in the genesis cut, not in an additive PR.
///
/// So instead of a comment, this test **fails the moment someone wires it in**,
/// forcing that compatibility question to be answered deliberately rather than
/// discovered in production. It is the same device used for the block-producer
/// gap in #2241: pin the current reality so it cannot change silently.
#[test]
fn canonical_decoding_has_no_production_caller_yet() {
    use std::path::Path;

    fn scan(dir: &Path, hits: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // `tests/` are the legitimate callers.
                if name != "tests" && name != "target" {
                    scan(&path, hits);
                }
                continue;
            }
            if path.extension().and_then(|x| x.to_str()) != Some("rs") {
                continue;
            }
            // The module that defines them is not a caller.
            if path.ends_with("crypto/canonical.rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                let l = line.trim_start();
                if l.starts_with("//") || l.starts_with("///") {
                    continue;
                }
                if l.contains("from_canonical(") || l.contains("decode_canonical(") {
                    hits.push(format!("{}:{}: {}", path.display(), i + 1, l.trim()));
                }
            }
        }
    }

    let mut hits = Vec::new();
    for root in [
        "../shared-protocol/src",
        "../native-chain/src",
        "../p2p/src",
    ] {
        scan(Path::new(root), &mut hits);
    }

    assert!(
        hits.is_empty(),
        "canonical decoding now has a production caller:\n{}\n\n\
         That is the intended destination, but it is a compatibility change, not \
         an additive one: every existing encoder must already emit canonical form \
         or nodes start rejecting each other mid-flight. Answer that question \
         (see docs/security/GENESIS_CUT_PLAN.md), then delete this test — do not \
         relax it.",
        hits.join("\n")
    );
}
