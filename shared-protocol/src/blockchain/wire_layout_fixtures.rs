//! #2883: golden byte fixtures for the bincode boundary types this crate owns.
//!
//! `Block` and `Transaction` cross process boundaries as positional bincode —
//! the block archive (`BLOCKS_TABLE`), block gossip (`NewBlock`), block sync
//! (`BlockResponse.blocks`), and transaction gossip. bincode encodes by shape
//! and order, not by name, so ANY field insertion, removal, or reorder is a
//! wire/storage format break (the #2727 hazard class). Three such breaks
//! shipped on unchanged identifiers in one month (#2884): `Block`
//! (`ae0a22833`), `FinalityVote` (`927e239ed`), `BlockResponse`
//! (`0bb151ff9`).
//!
//! Each fixture pins the exact bytes of a fully-populated value. Two
//! assertions per fixture:
//!
//! 1. `encode(value) == pinned bytes` — the layout tripwire. It also stays
//!    load-bearing if someone later adds a tolerant/lenient DECODER: a
//!    decode-only fixture goes quietly green under such a change while old
//!    binaries can no longer read the new bytes.
//! 2. `re-encode(decode(pinned bytes)) == pinned bytes` — old bytes still
//!    decode, and to the same canonical encoding.
//!
//! **If a fixture test brought you here:** you changed the serialized layout
//! of a boundary type. That is a format break for every peer and every
//! persisted byte. Decide the versioning explicitly — bump
//! [`WIRE_FORMAT_VERSION`](../../../p2p/src/message.rs) for wire types (the
//! identifier tripwire then forces the topic/protocol renames), or the
//! storage envelope version for persisted types (#2881's checkpoint pattern)
//! — and regenerate the fixture IN THE SAME PR with the failure output's
//! actual-bytes hex. Never regenerate without the version decision: that is
//! exactly the silent break this test exists to prevent.
//!
//! The `every_required_boundary_type_has_a_fixture` gate pins the covered set
//! BY IDENTITY (per #2883 review condition): removing a fixture requires
//! editing the literal `REQUIRED` list in the same diff, so the floor cannot
//! be lowered silently.

use chrono::TimeZone;

use super::block::Block;
use super::transaction::{
    Operation, Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use crate::common::types::Address;
use crate::crypto::hash::{Hash, HashType};

fn fixed_hash(byte: u8) -> Hash {
    Hash::new([byte; 32], HashType::Blake3)
}

fn fixture_block() -> Block {
    let mut block = Block::new(3, fixed_hash(0xA1), Vec::new(), 7, 42);
    block.nonce = 99;
    block.timestamp = chrono::Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
    block
        .metadata
        .insert("consensus_type".to_string(), "pop".to_string());
    block.producer_pubkey = [0x07; 32];
    block.producer_sig = vec![0x09; 64];
    block.hash = Some(fixed_hash(0xB2));
    block
}

fn fixture_transaction_coinbase() -> Transaction {
    let mut tx = Transaction::new_coinbase("0x0000000000000000000000000000000000000001", 50, 1);
    // new_coinbase stamps Utc::now() into both the timestamp and the id hash;
    // every wire field must be deterministic, so both are pinned here.
    tx.timestamp = chrono::Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
    tx.id = Some(fixed_hash(0xE5));
    tx
}

/// Every `Option` populated and every field value distinct, so a silent
/// same-type field swap cannot re-encode to the same bytes.
fn fixture_transaction_full() -> Transaction {
    let mut tx = Transaction::new_coinbase("0x0000000000000000000000000000000000000002", 1, 2);
    tx.id = Some(fixed_hash(0xC3));
    tx.tx_type = TransactionType::Transfer;
    tx.version = 2;
    tx.timestamp = chrono::Utc.timestamp_millis_opt(1_700_000_111_000).unwrap();
    tx.inputs = vec![TransactionInput {
        txid: fixed_hash(0xD4),
        vout: 5,
        signature: Some("sig-hex".to_string()),
        public_key: Some("pk-hex".to_string()),
        coinbase_data: None,
    }];
    tx.outputs = vec![TransactionOutput {
        amount: 61,
        address: "0xrecipient".to_string(),
        script: "script".to_string(),
        nft_data: None,
    }];
    tx.data = Some("{\"k\":1}".to_string());
    tx.lock_time = Some(77);
    tx.fee = 3;
    tx.sender_address = Address([0x11; 20]);
    tx.recipient_address = Some(Address([0x22; 20]));
    tx.amount = 58;
    tx.operation = Operation::Transfer { amount: 58 };
    tx.nonce = 8;
    tx
}

struct FixtureCase {
    /// Stable identity; the `REQUIRED` gate pins this list.
    name: &'static str,
    /// Pinned canonical bincode encoding, hex. Regenerate ONLY together with
    /// an explicit format-version decision (see module doc).
    expected_hex: &'static str,
    encode: fn() -> Vec<u8>,
    /// Decode the pinned bytes, then re-encode them.
    reencode: fn(&[u8]) -> Vec<u8>,
}

fn cases() -> Vec<FixtureCase> {
    vec![
        FixtureCase {
            name: "Block(signed,producer-auth shape)",
            expected_hex: "0300000002000000a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a10000000000000000070000002a0000000000000063000000000000001400000000000000323032332d31312d31345432323a31333a32305a01000000000000000e00000000000000636f6e73656e7375735f747970650300000000000000706f7007070707070707070707070707070707070707070707070707070707070707074000000000000000090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090909090102000000b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2",
            encode: || bincode::serialize(&fixture_block()).expect("encode Block"),
            reencode: |bytes| {
                let block: Block = bincode::deserialize(bytes).expect("decode pinned Block");
                bincode::serialize(&block).expect("re-encode Block")
            },
        },
        FixtureCase {
            name: "Transaction(coinbase-minimal)",
            expected_hex: "0102000000e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e501000000010000001400000000000000323032332d31312d31345432323a31333a32305a0100000000000000010000000000000000000000000000000000000000000000000000000000000000000000ffffffff0000010a00000000000000636f696e626173653a31010000000000000032000000000000002a0000000000000030783030303030303030303030303030303030303030303030303030303030303030303030303030303107000000000000004f505f54525545000000000000000000000000000000000000000000000000000000000000000032000000000000000000000032000000000000000000000000000000",
            encode: || {
                bincode::serialize(&fixture_transaction_coinbase()).expect("encode Transaction")
            },
            reencode: |bytes| {
                let tx: Transaction =
                    bincode::deserialize(bytes).expect("decode pinned Transaction");
                bincode::serialize(&tx).expect("re-encode Transaction")
            },
        },
        FixtureCase {
            name: "Transaction(all-options-populated)",
            expected_hex: "0102000000c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c302000000020000001400000000000000323032332d31312d31345432323a31353a31315a010000000000000002000000d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4050000000107000000000000007369672d686578010600000000000000706b2d6865780001000000000000003d000000000000000b000000000000003078726563697069656e740600000000000000736372697074000107000000000000007b226b223a317d014d00000000000000030000000000000011111111111111111111111111111111111111110122222222222222222222222222222222222222223a00000000000000000000003a000000000000000800000000000000",
            encode: || bincode::serialize(&fixture_transaction_full()).expect("encode Transaction"),
            reencode: |bytes| {
                let tx: Transaction =
                    bincode::deserialize(bytes).expect("decode pinned Transaction");
                bincode::serialize(&tx).expect("re-encode Transaction")
            },
        },
    ]
}

/// Identity-pinned coverage floor (#2883 review condition: identities, not
/// counts). Removing a fixture is a RED test until this literal list is
/// edited in the same, reviewable diff.
const REQUIRED: &[&str] = &[
    "Block(signed,producer-auth shape)",
    "Transaction(coinbase-minimal)",
    "Transaction(all-options-populated)",
];

#[test]
fn wire_layout_fixture_bytes_are_pinned() {
    for case in cases() {
        let actual = (case.encode)();
        let actual_hex = hex::encode(&actual);
        assert_eq!(
            actual_hex, case.expected_hex,
            "[{}] serialized layout changed — this is a wire/storage format break \
             (#2727 class, #2883). Decide the version bump explicitly, then \
             regenerate this fixture in the same PR. Actual bytes above.",
            case.name
        );
        let pinned = hex::decode(case.expected_hex).expect("fixture hex is valid");
        let reencoded = (case.reencode)(&pinned);
        assert_eq!(
            hex::encode(&reencoded),
            case.expected_hex,
            "[{}] pinned bytes decode, but re-encode to different bytes — the \
             decoder accepts an encoding the encoder no longer produces (an \
             asymmetric format change; old peers cannot read the new bytes)",
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
             floor is pinned by identity; a fixture may only be removed by editing \
             REQUIRED in the same diff, with the format-version decision recorded"
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

/// bincode (FixintEncoding) writes enum discriminants as u32 LE. A mid-enum
/// variant insertion renumbers every later variant — old bytes then decode as
/// the WRONG variant or refuse. This pins every `TransactionType`
/// discriminant by literal value, so an insertion anywhere but the end is an
/// immediate RED naming the shifted variants. (The `WrfcoinMessage` enum has
/// the same protection via `kind_from_wire`, #2115.)
#[test]
fn transaction_type_discriminants_are_pinned() {
    use TransactionType as T;
    let pinned: &[(T, u32)] = &[
        (T::Regular, 0),
        (T::Coinbase, 1),
        (T::Transfer, 2),
        (T::Stake, 3),
        (T::Unstake, 4),
        (T::NftCreation, 5),
        (T::NftTransfer, 6),
        (T::NftExecution, 7),
        (T::DataAnalysis, 8),
        (T::DataCollection, 9),
        (T::DataStorage, 10),
        (T::EnvironmentalOffset, 11),
        (T::BridgeLock, 12),
        (T::BridgeUnlock, 13),
        (T::ContributionCommit, 14),
        (T::ContributionReveal, 15),
        (T::ForecastCommit, 16),
        (T::ForecastReveal, 17),
        (T::OracleAttestation, 18),
        (T::GovernanceProposal, 19),
        (T::GovernanceVote, 20),
        (T::GovernanceExecute, 21),
        (T::GovernanceFinalize, 22),
        (T::GovernanceDelegate, 23),
        (T::TreasuryDeposit, 24),
        (T::TreasuryWithdraw, 25),
        (T::Reward, 26),
        (T::ConfidentialTransfer, 27),
        (T::ShieldedDeposit, 28),
        (T::ShieldedWithdrawal, 29),
        (T::Layer2BatchSubmit, 30),
        (T::RewardClaim, 31),
        (T::SlashingReport, 32),
        (T::OracleRegistration, 33),
        (T::StationRegistration, 34),
        (T::FeatureScheduleExtension, 35),
        (T::SlashEvidenceReport, 36),
        (T::NodeBondLifecycle, 37),
        (T::NodeServiceBatch, 38),
        (T::NodeFraudReport, 39),
        (T::SessionKeyAction, 40),
    ];
    assert_eq!(
        pinned.len(),
        41,
        "TransactionType variant count changed — new variants go at the END \
         (append-only); add the new variant to this table with the next index"
    );
    for (variant, index) in pinned {
        let bytes = bincode::serialize(variant).expect("encode TransactionType");
        assert_eq!(
            bytes,
            index.to_le_bytes().to_vec(),
            "TransactionType::{variant:?} discriminant moved from {index} — a \
             mid-enum insertion renumbers every later variant on the wire and \
             in every persisted transaction (#2727 class, #2883); new variants \
             are append-only"
        );
    }
}
