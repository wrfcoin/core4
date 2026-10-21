//! #2883: golden byte fixtures for the bincode boundary types this crate owns.
//!
//! Every [`WrfcoinMessage`] variant travels the gossip mesh, and
//! [`BlockRequest`]/[`BlockResponse`] travel `/wrfcoin/block-sync` — all as
//! positional bincode read from other nodes' bytes. `BlockResponse` shipped
//! the append break in the audited window (`0bb151ff9`, `retry_after_ms`)
//! with its own doc comment calling it breaking while the protocol string
//! stayed put (#2884).
//!
//! Mechanism, failure protocol, and the identity-pinned coverage floor are
//! documented in `shared-protocol/src/blockchain/wire_layout_fixtures.rs` —
//! this module is its per-crate sibling. One fixture per `WrfcoinMessage`
//! variant pins each variant's payload layout; variant ORDER is already
//! pinned by the `kind_from_wire` discriminant table (#2115), and the topic
//! and protocol identifiers by `wire_identifiers_carry_the_wire_format_version`
//! (#2884). If a test here brought you here: bump
//! [`crate::message::WIRE_FORMAT_VERSION`], rename the identifiers (that
//! tripwire forces it), and regenerate the fixture in the same PR.

use crate::da_challenge::{DataAvailabilityChallenge, DataAvailabilityChallengeResponse};
use crate::message::{ContentRef, WrfcoinMessage};
use crate::sync::{BlockRequest, BlockResponse};

fn content_ref() -> ContentRef {
    ContentRef {
        data_cid: "ipfs:QmFixture".to_string(),
        data_hash: "ab".repeat(32),
        storage_provider: "ipfs".to_string(),
        data_size: 4096,
    }
}

fn message_fixtures() -> Vec<(&'static str, WrfcoinMessage)> {
    vec![
        (
            "WrfcoinMessage::NewBlock",
            WrfcoinMessage::NewBlock {
                block_bytes: vec![0xB0, 0xB1, 0xB2],
            },
        ),
        (
            "WrfcoinMessage::NewTransactions",
            WrfcoinMessage::NewTransactions {
                tx_bytes: vec![vec![0x71], vec![0x72, 0x73]],
            },
        ),
        (
            "WrfcoinMessage::WeatherData",
            WrfcoinMessage::WeatherData {
                station_id: "station-9".to_string(),
                data: vec![0x10, 0x20],
                timestamp: 1_700_000_333,
                nonce: Some("0011223344556677".to_string()),
                station_signature: "aabb".to_string(),
                station_pubkey: "ccdd".to_string(),
            },
        ),
        (
            "WrfcoinMessage::ChainStatus",
            WrfcoinMessage::ChainStatus {
                height: 1234,
                tip_hash: [0x35; 32],
                state_root: [0x46; 32],
            },
        ),
        (
            "WrfcoinMessage::ForecastTaskAssignment",
            WrfcoinMessage::ForecastTaskAssignment {
                task_id: "task-1".to_string(),
                node_id: "node-2".to_string(),
                manifest_bytes: vec![0x51, 0x52],
            },
        ),
        (
            "WrfcoinMessage::ComputeNodeHeartbeat",
            WrfcoinMessage::ComputeNodeHeartbeat {
                node_id: "node-3".to_string(),
                capabilities: vec!["wrf".to_string()],
                region: "us-test-1".to_string(),
                active_tasks: 2,
                max_concurrent_tasks: 9,
                observation_density: Some(0.5),
                station_count: Some(11),
            },
        ),
        (
            "WrfcoinMessage::ForecastTaskResult",
            WrfcoinMessage::ForecastTaskResult {
                task_id: "task-4".to_string(),
                node_id: "node-5".to_string(),
                receipt_bytes: vec![0x61],
            },
        ),
        (
            "WrfcoinMessage::WeatherDataV2",
            WrfcoinMessage::WeatherDataV2 {
                station_id: "station-v2".to_string(),
                data: Vec::new(),
                timestamp: 1_700_000_444,
                content_ref: Some(content_ref()),
                nonce: "8899aabbccddeeff".to_string(),
                station_signature: "eeff".to_string(),
                station_pubkey: "0011".to_string(),
            },
        ),
        (
            "WrfcoinMessage::ForecastTaskResultV2",
            WrfcoinMessage::ForecastTaskResultV2 {
                task_id: "task-6".to_string(),
                node_id: "node-7".to_string(),
                receipt_bytes: Vec::new(),
                content_ref: Some(content_ref()),
            },
        ),
        (
            "WrfcoinMessage::DaChallenge",
            WrfcoinMessage::DaChallenge {
                challenge: DataAvailabilityChallenge {
                    challenge_id: "challenge-8".to_string(),
                    cid: "ipfs:QmTarget".to_string(),
                    nonce: [0x5A; 32],
                    offset: 4096,
                    length: 2048,
                    issued_at: 1_700_000_555,
                },
            },
        ),
        (
            "WrfcoinMessage::DaChallengeResponse",
            WrfcoinMessage::DaChallengeResponse {
                response: DataAvailabilityChallengeResponse {
                    challenge_id: "challenge-8".to_string(),
                    data_fragment: vec![0x91, 0x92],
                    proof: [0x6B; 32],
                    keeper_id: "keeper-9".to_string(),
                    responded_at: 1_700_000_566,
                },
            },
        ),
        (
            "WrfcoinMessage::GenesisHandshake",
            WrfcoinMessage::GenesisHandshake {
                network_id: "testnet".to_string(),
                chain_id: "wrfcoin-test".to_string(),
                genesis_hash: "cd".repeat(32),
                protocol_version: 2,
            },
        ),
        (
            "WrfcoinMessage::FinalityVote",
            WrfcoinMessage::FinalityVote {
                vote_bytes: vec![0xF1, 0xF2],
            },
        ),
        (
            "WrfcoinMessage::FinalityCertificate",
            WrfcoinMessage::FinalityCertificate {
                cert_bytes: vec![0xF3],
            },
        ),
    ]
}

struct FixtureCase {
    name: &'static str,
    expected_hex: &'static str,
    encode: fn() -> Vec<u8>,
    reencode: fn(&[u8]) -> Vec<u8>,
}

fn sync_cases() -> Vec<FixtureCase> {
    vec![
        FixtureCase {
            name: "BlockRequest",
            expected_hex: "bc1b00000000000040000000",
            encode: || {
                bincode::serialize(&BlockRequest {
                    start_height: 7100,
                    count: 64,
                })
                .expect("encode BlockRequest")
            },
            reencode: |bytes| {
                let req: BlockRequest =
                    bincode::deserialize(bytes).expect("decode pinned BlockRequest");
                bincode::serialize(&req).expect("re-encode BlockRequest")
            },
        },
        FixtureCase {
            name: "BlockResponse(retry_after_ms shape, #2539)",
            expected_hex: "000000000000000001c409000000000000",
            encode: || {
                bincode::serialize(&BlockResponse {
                    blocks: Vec::new(),
                    retry_after_ms: Some(2500),
                })
                .expect("encode BlockResponse")
            },
            reencode: |bytes| {
                let resp: BlockResponse =
                    bincode::deserialize(bytes).expect("decode pinned BlockResponse");
                bincode::serialize(&resp).expect("re-encode BlockResponse")
            },
        },
    ]
}

/// Pinned canonical encodings for every `WrfcoinMessage` variant, in
/// declaration order. Filled by the fixture-generation protocol in the
/// module doc; the names double as the identity-pinned coverage floor.
const MESSAGE_EXPECTED_HEX: &[(&str, &str)] = &[
    ("WrfcoinMessage::NewBlock", "000000000300000000000000b0b1b2"),
    ("WrfcoinMessage::NewTransactions", "01000000020000000000000001000000000000007102000000000000007273"),
    ("WrfcoinMessage::WeatherData", "02000000090000000000000073746174696f6e2d39020000000000000010204df253650000000001100000000000000030303131323233333434353536363737040000000000000061616262040000000000000063636464"),
    ("WrfcoinMessage::ChainStatus", "03000000d20400000000000035353535353535353535353535353535353535353535353535353535353535354646464646464646464646464646464646464646464646464646464646464646"),
    ("WrfcoinMessage::ForecastTaskAssignment", "0400000006000000000000007461736b2d3106000000000000006e6f64652d3202000000000000005152"),
    ("WrfcoinMessage::ComputeNodeHeartbeat", "0500000006000000000000006e6f64652d3301000000000000000300000000000000777266090000000000000075732d746573742d31020000000900000001000000000000e03f010b000000"),
    ("WrfcoinMessage::ForecastTaskResult", "0600000006000000000000007461736b2d3406000000000000006e6f64652d35010000000000000061"),
    ("WrfcoinMessage::WeatherDataV2", "070000000a0000000000000073746174696f6e2d76320000000000000000bcf2536500000000010e00000000000000697066733a516d466978747572654000000000000000616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261620400000000000000697066730010000000000000100000000000000038383939616162626363646465656666040000000000000065656666040000000000000030303131"),
    ("WrfcoinMessage::ForecastTaskResultV2", "0800000006000000000000007461736b2d3606000000000000006e6f64652d370000000000000000010e00000000000000697066733a516d466978747572654000000000000000616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261620400000000000000697066730010000000000000"),
    ("WrfcoinMessage::DaChallenge", "090000000b000000000000006368616c6c656e67652d380d00000000000000697066733a516d5461726765745a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a0010000000000000000800002bf3536500000000"),
    ("WrfcoinMessage::DaChallengeResponse", "0a0000000b000000000000006368616c6c656e67652d38020000000000000091926b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b08000000000000006b65657065722d3936f3536500000000"),
    ("WrfcoinMessage::GenesisHandshake", "0b0000000700000000000000746573746e65740c00000000000000777266636f696e2d7465737440000000000000006364636463646364636463646364636463646364636463646364636463646364636463646364636463646364636463646364636463646364636463646364636402000000"),
    ("WrfcoinMessage::FinalityVote", "0c0000000200000000000000f1f2"),
    ("WrfcoinMessage::FinalityCertificate", "0d0000000100000000000000f3"),
];

/// Identity-pinned coverage floor for the sync pair.
const SYNC_REQUIRED: &[&str] = &["BlockRequest", "BlockResponse(retry_after_ms shape, #2539)"];

#[test]
fn wrfcoin_message_variant_layouts_are_pinned() {
    let fixtures = message_fixtures();
    assert_eq!(
        fixtures.len(),
        MESSAGE_EXPECTED_HEX.len(),
        "WrfcoinMessage variant count changed — new variants are append-only \
         (see kind_from_wire, #2115) and need a fixture + expected-hex entry here"
    );
    for ((name, message), (pinned_name, pinned_hex)) in
        fixtures.iter().zip(MESSAGE_EXPECTED_HEX.iter())
    {
        assert_eq!(name, pinned_name, "fixture order must match the pinned table");
        let actual = message.to_bytes().expect("encode WrfcoinMessage");
        assert_eq!(
            hex::encode(&actual),
            *pinned_hex,
            "[{name}] serialized layout changed — a wire format break (#2727 \
             class, #2883). Bump WIRE_FORMAT_VERSION, rename the identifiers \
             (the #2884 tripwire forces it), and regenerate this fixture in \
             the same PR. Actual bytes above."
        );
        let pinned = hex::decode(pinned_hex).expect("fixture hex is valid");
        let decoded = WrfcoinMessage::from_bytes(&pinned).expect("decode pinned WrfcoinMessage");
        assert_eq!(
            hex::encode(decoded.to_bytes().expect("re-encode")),
            *pinned_hex,
            "[{name}] pinned bytes decode but re-encode differently — an \
             asymmetric format change; old peers cannot read the new bytes"
        );
    }
}

#[test]
fn block_sync_message_layouts_are_pinned() {
    for case in sync_cases() {
        let actual_hex = hex::encode((case.encode)());
        assert_eq!(
            actual_hex, case.expected_hex,
            "[{}] serialized layout changed — a wire format break on \
             /wrfcoin/block-sync (#2727 class, #2883). Bump \
             WIRE_FORMAT_VERSION, rename the protocol id, and regenerate this \
             fixture in the same PR. Actual bytes above.",
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
    let have: Vec<&str> = sync_cases().iter().map(|c| c.name).collect();
    for required in SYNC_REQUIRED {
        assert!(
            have.contains(required),
            "boundary type fixture [{required}] is missing — the #2883 coverage \
             floor is pinned by identity and may only shrink by editing \
             SYNC_REQUIRED in the same diff"
        );
    }
    // The WrfcoinMessage floor is the MESSAGE_EXPECTED_HEX table itself: the
    // variant-count assertion plus per-name matching make removal of any
    // per-variant fixture a RED test until the literal table is edited.
    let names: Vec<&str> = message_fixtures().iter().map(|(n, _)| *n).collect();
    for (pinned_name, _) in MESSAGE_EXPECTED_HEX {
        assert!(
            names.contains(pinned_name),
            "WrfcoinMessage fixture [{pinned_name}] is missing — pinned by identity"
        );
    }
}
