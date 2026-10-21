//! #2883/#2913: golden byte fixtures for the bincode boundary types this
//! crate owns.
//!
//! [`ForecastArtifactManifest`] and [`ForecastVerificationReceipt`] cross a
//! real peer boundary as positional bincode: the launch API serializes the
//! manifest into `WrfcoinMessage::ForecastTaskAssignment::manifest_bytes`,
//! and peers deserialize both it and the receipt carried in
//! `ForecastTaskResult(V2)::receipt_bytes` (`p2p_events.rs`). The p2p-crate
//! fixtures pin only the ENVELOPE — those fields are opaque `Vec<u8>` there —
//! so the payload layout must be pinned here, beside the type definitions
//! (the #2913 review demonstrated a `#[serde(skip)]` change to this boundary
//! leaving every envelope fixture green).
//!
//! Mechanism, failure protocol, and the identity-pinned coverage floor are
//! documented in `shared-protocol/src/blockchain/wire_layout_fixtures.rs` —
//! this module is its per-crate sibling. If a test here brought you here:
//! bump [`WIRE_FORMAT_VERSION`] (these are wire payloads), let its tripwire
//! force the identifier renames, and regenerate the fixture in the same PR.

use crate::ForecastAccuracyMetrics;
use crate::blockchain::{ForecastArtifactManifest, ForecastVerificationReceipt};

fn fixture_manifest() -> ForecastArtifactManifest {
    ForecastArtifactManifest {
        schema_version: 2,
        model_id: "wrf-nano-v1".to_string(),
        input_hash: "ab".repeat(32),
        output_hash: "cd".repeat(32),
        produced_at: 1_700_000_777,
        region: "conus-east".to_string(),
        horizon_hours: 48,
        input_variables: vec!["t2m".to_string(), "u10".to_string()],
        output_variables: vec!["precip".to_string()],
    }
}

/// Both `Option`s populated: with `skip_serializing_if` on them, the
/// both-`Some` shape is the only bincode encoding of this struct that can
/// round-trip at all (a `None` encoding omits the `Option` tag byte the
/// positional decoder unconditionally expects — the `BlockResponse`
/// doc-comment hazard, live in this type).
fn fixture_receipt() -> ForecastVerificationReceipt {
    ForecastVerificationReceipt {
        manifest_digest: "ef".repeat(32),
        commit_id: "commit-77".to_string(),
        oracle_id: "oracle-3".to_string(),
        metrics: ForecastAccuracyMetrics {
            mae: 0.25,
            rmse: 0.5,
            bias: -0.125,
            skill_score: 0.75,
        },
        verified_at: 1_700_000_888,
        nonce: "0011223344556677".to_string(),
        zk_proof: Some("99aa".to_string()),
        signature: Some("bbcc".to_string()),
    }
}

/// Both `Option`s `None`: the shape #2922 fixed. `skip_serializing_if`
/// omitted the tag byte the positional decoder unconditionally expects,
/// making this exact shape undecodable — the ordinary shape for a receipt
/// with no ZK proof and no signature yet, not a rare one. Registered
/// alongside the both-`Some` fixture above by identity (`REQUIRED`) so this
/// coverage floor cannot silently regress back to Some-only.
fn fixture_receipt_none() -> ForecastVerificationReceipt {
    ForecastVerificationReceipt {
        manifest_digest: "12".repeat(32),
        commit_id: "commit-91".to_string(),
        oracle_id: "oracle-7".to_string(),
        metrics: ForecastAccuracyMetrics {
            mae: 0.11,
            rmse: 0.22,
            bias: -0.33,
            skill_score: 0.44,
        },
        verified_at: 1_700_000_999,
        nonce: "7766554433221100".to_string(),
        zk_proof: None,
        signature: None,
    }
}

struct FixtureCase {
    name: &'static str,
    expected_hex: &'static str,
    encode: fn() -> Vec<u8>,
    reencode: fn(&[u8]) -> Vec<u8>,
}

fn cases() -> Vec<FixtureCase> {
    vec![
        FixtureCase {
            name: "ForecastArtifactManifest",
            expected_hex: "020000000b000000000000007772662d6e616e6f2d763140000000000000006162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616240000000000000006364636463646364636463646364636463646364636463646364636463646364636463646364636463646364636463646364636463646364636463646364636409f45365000000000a00000000000000636f6e75732d65617374300000000200000000000000030000000000000074326d030000000000000075313001000000000000000600000000000000707265636970",
            encode: || bincode::serialize(&fixture_manifest()).expect("encode manifest"),
            reencode: |bytes| {
                let manifest: ForecastArtifactManifest =
                    bincode::deserialize(bytes).expect("decode pinned manifest");
                assert_eq!(
                    manifest,
                    fixture_manifest(),
                    "pinned manifest bytes decoded to a DIFFERENT value — a \
                     silent field swap or representation change (#2913)"
                );
                bincode::serialize(&manifest).expect("re-encode manifest")
            },
        },
        FixtureCase {
            name: "ForecastVerificationReceipt(both-options-Some)",
            expected_hex: "4000000000000000656665666566656665666566656665666566656665666566656665666566656665666566656665666566656665666566656665666566656665666566656665660900000000000000636f6d6d69742d373708000000000000006f7261636c652d33000000000000d03f000000000000e03f000000000000c0bf000000000000e83f78f45365000000001000000000000000303031313232333334343535363637370104000000000000003939616101040000000000000062626363",
            encode: || bincode::serialize(&fixture_receipt()).expect("encode receipt"),
            reencode: |bytes| {
                let receipt: ForecastVerificationReceipt =
                    bincode::deserialize(bytes).expect("decode pinned receipt");
                assert_eq!(
                    receipt,
                    fixture_receipt(),
                    "pinned receipt bytes decoded to a DIFFERENT value — a \
                     silent field swap or representation change (#2913)"
                );
                bincode::serialize(&receipt).expect("re-encode receipt")
            },
        },
        FixtureCase {
            name: "ForecastVerificationReceipt(both-options-None)",
            expected_hex: "4000000000000000313231323132313231323132313231323132313231323132313231323132313231323132313231323132313231323132313231323132313231323132313231320900000000000000636f6d6d69742d393108000000000000006f7261636c652d37295c8fc2f528bc3f295c8fc2f528cc3f1f85eb51b81ed5bf295c8fc2f528dc3fe7f45365000000001000000000000000373736363535343433333232313130300000",
            encode: || bincode::serialize(&fixture_receipt_none()).expect("encode receipt"),
            reencode: |bytes| {
                let receipt: ForecastVerificationReceipt =
                    bincode::deserialize(bytes).expect("decode pinned receipt");
                assert_eq!(
                    receipt,
                    fixture_receipt_none(),
                    "pinned receipt bytes decoded to a DIFFERENT value — a \
                     silent field swap or representation change (#2922)"
                );
                bincode::serialize(&receipt).expect("re-encode receipt")
            },
        },
    ]
}

/// Identity-pinned coverage floor — see the shared-protocol sibling module.
const REQUIRED: &[&str] = &[
    "ForecastArtifactManifest",
    "ForecastVerificationReceipt(both-options-Some)",
    "ForecastVerificationReceipt(both-options-None)",
];

#[test]
fn wire_layout_fixture_bytes_are_pinned() {
    for case in cases() {
        let actual_hex = hex::encode((case.encode)());
        assert_eq!(
            actual_hex, case.expected_hex,
            "[{}] serialized layout changed — a wire format break for every \
             peer decoding forecast assignments/receipts (#2727 class, #2883, \
             #2913). Bump WIRE_FORMAT_VERSION, rename the identifiers, and \
             regenerate this fixture in the same PR. Actual bytes above.",
            case.name
        );
        let pinned = hex::decode(case.expected_hex).expect("fixture hex is valid");
        let reencoded = (case.reencode)(&pinned);
        assert_eq!(
            hex::encode(&reencoded),
            case.expected_hex,
            "[{}] pinned bytes decode but re-encode differently — an \
             asymmetric format change; old peers cannot read the new bytes",
            case.name
        );
    }
}

#[test]
fn every_required_boundary_type_has_a_fixture() {
    let have: Vec<&str> = cases().iter().map(|c| c.name).collect();
    for required in REQUIRED {
        assert!(
            have.contains(required),
            "boundary type fixture [{required}] is missing — the #2883 coverage \
             floor is pinned by identity and may only shrink by editing REQUIRED \
             in the same diff"
        );
    }
    for name in &have {
        assert!(
            REQUIRED.contains(name),
            "fixture [{name}] is not registered in REQUIRED — add it so the \
             coverage floor owns it"
        );
    }
}
