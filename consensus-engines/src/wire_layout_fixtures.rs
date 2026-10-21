//! #2883: golden byte fixtures for the bincode boundary types this crate owns.
//!
//! `FinalityVote` and `FinalityCertificate` cross the network as opaque
//! bincode blobs inside gossip (`WrfcoinMessage::FinalityVote` /
//! `FinalityCertificate`); `EquivocationEvidence` is persisted to the
//! evidence redb by `native-chain`. bincode is positional, so any field
//! insertion, removal, or reorder breaks every peer and every persisted byte
//! (#2727 class). `FinalityVote` shipped exactly that break in the audited
//! window (`927e239ed`: `epoch` + `validator_set_hash` inserted mid-struct).
//!
//! Mechanism, failure protocol, and the identity-pinned coverage floor are
//! documented in `shared-protocol/src/blockchain/wire_layout_fixtures.rs` —
//! this module is its per-crate sibling. If a test here brought you here:
//! decide the format-version bump first (wire types →
//! `WIRE_FORMAT_VERSION`; persisted types → a storage envelope version),
//! then regenerate the fixture in the same PR.

use crate::equivocation::EquivocationEvidence;
use crate::selection::bft_finality::{FinalityCertificate, FinalityVote};
use wrfcoin_shared_protocol::crypto::hash::{Hash, HashType};

fn fixture_vote() -> FinalityVote {
    FinalityVote {
        block_hash: [0xA7; 32],
        block_height: 41,
        epoch: 6,
        validator_set_hash: [0xB8; 32],
        validator_id: "validator-one".to_string(),
        signature: vec![0x0C; 64],
    }
}

fn fixture_certificate() -> FinalityCertificate {
    FinalityCertificate {
        block_hash: [0xA7; 32],
        block_height: 41,
        votes: vec![fixture_vote()],
        total_vote_weight: 700,
        total_validator_weight: 1000,
    }
}

fn fixture_evidence() -> EquivocationEvidence {
    EquivocationEvidence {
        validator: "validator-two".to_string(),
        height: 55,
        first_block_hash: Hash::new([0xD1; 32], HashType::Blake3),
        second_block_hash: Hash::new([0xE2; 32], HashType::Blake3),
        detected_at: 1_700_000_222,
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
            name: "FinalityVote(epoch shape, #2256)",
            expected_hex: "a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a729000000000000000600000000000000b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b80d0000000000000076616c696461746f722d6f6e6540000000000000000c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c",
            encode: || bincode::serialize(&fixture_vote()).expect("encode FinalityVote"),
            reencode: |bytes| {
                let vote: FinalityVote =
                    bincode::deserialize(bytes).expect("decode pinned FinalityVote");
                bincode::serialize(&vote).expect("re-encode FinalityVote")
            },
        },
        FixtureCase {
            name: "FinalityCertificate(one-vote)",
            expected_hex: "a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a729000000000000000100000000000000a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a729000000000000000600000000000000b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b80d0000000000000076616c696461746f722d6f6e6540000000000000000c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0cbc020000000000000000000000000000e8030000000000000000000000000000",
            encode: || {
                bincode::serialize(&fixture_certificate()).expect("encode FinalityCertificate")
            },
            reencode: |bytes| {
                let cert: FinalityCertificate =
                    bincode::deserialize(bytes).expect("decode pinned FinalityCertificate");
                bincode::serialize(&cert).expect("re-encode FinalityCertificate")
            },
        },
        FixtureCase {
            name: "EquivocationEvidence",
            expected_hex: "0d0000000000000076616c696461746f722d74776f370000000000000002000000d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d102000000e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2e2def1536500000000",
            encode: || bincode::serialize(&fixture_evidence()).expect("encode EquivocationEvidence"),
            reencode: |bytes| {
                let ev: EquivocationEvidence =
                    bincode::deserialize(bytes).expect("decode pinned EquivocationEvidence");
                bincode::serialize(&ev).expect("re-encode EquivocationEvidence")
            },
        },
    ]
}

/// Identity-pinned coverage floor — see the shared-protocol sibling module.
const REQUIRED: &[&str] = &[
    "FinalityVote(epoch shape, #2256)",
    "FinalityCertificate(one-vote)",
    "EquivocationEvidence",
];

#[test]
fn wire_layout_fixture_bytes_are_pinned() {
    for case in cases() {
        let actual_hex = hex::encode((case.encode)());
        assert_eq!(
            actual_hex, case.expected_hex,
            "[{}] serialized layout changed — a wire/storage format break (#2727 \
             class, #2883). Decide the version bump explicitly, then regenerate \
             this fixture in the same PR. Actual bytes above.",
            case.name
        );
        let pinned = hex::decode(case.expected_hex).expect("fixture hex is valid");
        let reencoded = (case.reencode)(&pinned);
        assert_eq!(
            hex::encode(&reencoded),
            case.expected_hex,
            "[{}] pinned bytes decode but re-encode differently — an asymmetric \
             format change; old peers cannot read the new bytes",
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
