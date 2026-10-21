//! #3430 slice 2: gate-OFF baseline for the Stake/Unstake consensus prerequisite.
//!
//! SCOPE, STATED PLAINLY: this fixture calls the historical block-application seam directly:
//! `extract_transitions` (the peer/WAL extractor), `apply_transitions_batch_at_height`,
//! `apply_block_finalization`, and `governance_transitions_for_block` (the producer). It does NOT go
//! through mempool admission (`pool.rs:376` `local_admission_disabled`, the B3.1 refusal), routes,
//! gossip admission, full block validation or any network/WAL runtime. It therefore supplies no
//! evidence that new Stake submissions are admitted (they are not, today), only what already-valid
//! signed Stake/Unstake blocks do when applied.
//!
//! `stake_submission_nonce` does not exist yet. These tests pin what main does TODAY, with
//! pinned transaction and block timestamps so state roots AND block hashes are byte-stable:
//!
//! - KNOWN DEFECT (replay): a confirmed authorization is replayable. A rewrapped copy with a distinct
//!   transaction id locks the stake a second time and the account nonce stays 0.
//! - KNOWN DEFECT (producer/applier divergence): the producer emits no staking transition, so the
//!   producer root differs from the peer root for the same Stake.
//!
//! Both are BROKEN behaviour preserved on purpose: with the gate OFF, historical blocks must keep
//! applying exactly as they do now, so these goldens are changed ONLY by the gated feature, never by
//! an edit to the gate-OFF path. The gate-ON tests (a later slice) assert the corrected behaviour.
//! Own process: nothing here installs a feature other than the plain v1 schedule.

mod helpers {
    pub(crate) mod test_env {
        // Mount the canonical test-only lock and fixture pins (#3521).
        include!("../src/bin/launch_api/test_env.rs");
    }
}

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;

use chrono::{DateTime, Utc};
use rand::SeedableRng;
use rand::rngs::StdRng;

use transition_extractor::extract_transitions;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::transaction::{
    StakeAuthPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, install_activation_table,
    install_feature_schedule, resolve_activation_schedule, resolve_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const FUNDED: u128 = 10_000;
const AMOUNT: u64 = 2_000;
/// Pinned fixture clock: transaction and block constructors default to the wall clock, which would
/// make every id, Merkle root and block hash differ per run.
const FIXTURE_UNIX: i64 = 1_700_000_000;

fn fixture_time(offset: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(FIXTURE_UNIX + offset, 0).expect("fixture timestamp")
}

fn install_plain_v1() {
    let raw = "0:v1";
    let pairs = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, raw)];
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
        .expect("activation table");
    let features = resolve_feature_schedule(pairs, &table).expect("feature schedule");
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
}

fn staker() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3430))
}

fn staker_address() -> Address {
    Address::from_public_key(&PublicKey::from_private_key(&staker()))
}

fn genesis() -> State {
    let mut state = State::new();
    state.create_account(staker_address(), Balance::new(FUNDED));
    state.recalculate_state_root();
    state
}

/// A signed Stake/Unstake; `wrap` varies the (unsigned) input txid so a copy has a distinct
/// transaction id with the identical authorization.
fn signed(is_unstake: bool, nonce: u64, wrap: &str) -> Transaction {
    signed_at(is_unstake, nonce, wrap, 0)
}

fn signed_at(is_unstake: bool, nonce: u64, wrap: &str, tx_offset: i64) -> Transaction {
    let key = staker();
    let account = staker_address();
    let mut payload = StakeAuthPayload {
        account: account.to_hex(),
        amount: AMOUNT,
        auth_pubkey: None,
        auth_signature: None,
    };
    payload.sign_authorization(&key, is_unstake, nonce);
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("{wrap}-{}", account.to_hex())),
            vout: 0,
            signature: None,
            public_key: Some(account.to_hex()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: AMOUNT,
            address: account.to_hex(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = if is_unstake {
        TransactionType::Unstake
    } else {
        TransactionType::Stake
    };
    tx.sender_address = account;
    tx.nonce = nonce;
    tx.data = Some(serde_json::to_string(&payload).expect("payload json"));
    tx.timestamp = fixture_time(tx_offset);
    tx.set_id();
    tx
}

#[derive(Debug, PartialEq, Eq)]
struct Step {
    height: u64,
    locked: u128,
    spendable: u128,
    nonce: u64,
    root: String,
    /// Hash of the block applied at this height (empty for genesis).
    block_hash: String,
}

fn snapshot(state: &State, height: u64) -> Step {
    let account = state
        .get_account(&staker_address())
        .expect("staker account");
    Step {
        height,
        locked: account.stake_locked.into_inner(),
        spendable: account.spendable_balance.into_inner(),
        nonce: account.nonce,
        root: state.state_root.to_string(),
        block_hash: String::new(),
    }
}

/// Apply `txs` at `height` through the peer extractor, as a syncing node or WAL replay does.
fn apply_peer(state: &mut State, prev: &Hash, height: u64, txs: Vec<Transaction>) -> Hash {
    let mut block = Block::new(1, prev.clone(), txs, 1, height);
    block.timestamp = fixture_time(height as i64);
    let extracted = extract_transitions(state, &block, height, block.timestamp.timestamp());
    state
        .apply_transitions_batch_at_height(height, extracted.into_iter())
        .unwrap_or_else(|e| panic!("peer applies height {height}: {e}"));
    state.apply_block_finalization(height);
    block.calculate_hash()
}

fn peer_chain() -> Vec<Step> {
    install_plain_v1();
    let mut state = genesis();
    let mut prev = Hash::default();
    let mut steps = vec![snapshot(&state, 0)];
    let plan: Vec<(u64, Vec<Transaction>)> = vec![
        (1, vec![signed(false, 0, "first")]),
        // Rewrapped copy of the confirmed authorization: distinct id, same signature.
        (2, vec![signed(false, 0, "rewrapped")]),
        (3, vec![signed(true, 1, "release")]),
    ];
    for (height, txs) in plan {
        prev = apply_peer(&mut state, &prev, height, txs);
        let mut step = snapshot(&state, height);
        step.block_hash = prev.to_string();
        steps.push(step);
    }
    steps
}

/// KNOWN-DEFECT BASELINE (replay): the rewrapped copy at height 2 locks AGAIN and the nonce never
/// moves. This is wrong behaviour, preserved for the gate-OFF historical path; only the gated
/// `stake_submission_nonce` feature may change it (its gate-ON tests assert the corrected result).
#[test]
fn known_defect_baseline_replayed_stake_double_locks_and_nonce_stays_zero() {
    let steps = peer_chain();
    let observed: Vec<(u64, u128, u128, u64)> = steps
        .iter()
        .map(|s| (s.height, s.locked, s.spendable, s.nonce))
        .collect();
    assert_eq!(
        observed,
        vec![
            (0, 0, FUNDED, 0),
            // Stake locks 2,000; no nonce is consumed.
            (1, 2_000, 8_000, 0),
            // The rewrapped replay locks again (the defect the gated feature removes).
            (2, 4_000, 6_000, 0),
            // Unstake (nonce 1 in its authorization) releases 2,000; the nonce is still 0.
            (3, 2_000, 8_000, 0),
        ],
        "gate-OFF peer-path behaviour must stay exactly main's"
    );
}

/// State roots after genesis and each block, pinned from unmodified main (core4 `1b908b384`). Heights 1
/// and 3 share a root because the account state is identical (2,000 locked, nonce 0): the root has no
/// memory of the replayed Stake in between, which is exactly the persistent-replay gap. Every gated
/// slice must reproduce these byte for byte with `stake_submission_nonce` OFF.
const GOLDEN_PEER_ROOTS: [&str; 4] = [
    "98f20d4d1fc2884fb63a33499cab10b92ab0a362e46c7e64e6cf582d663b7caa", // pragma: allowlist secret
    "c9d2765ade01605adb2f61741bbb72a1a4d8fe825975fc12f53083df439d194b", // pragma: allowlist secret
    "2e83e3b22f112e49803eb3586256ce7b3ea07d88ac10f781e9865ee652169b6a", // pragma: allowlist secret
    "c9d2765ade01605adb2f61741bbb72a1a4d8fe825975fc12f53083df439d194b", // pragma: allowlist secret
];

/// Block hashes at heights 1-3 (pinned transaction and block timestamps), pinned from unmodified main.
const GOLDEN_PEER_BLOCK_HASHES: [&str; 3] = [
    "0dd5a1d12eb347876d234f0fd32b2e0cf666d30a6ec1df36095f31d9d89655cc", // pragma: allowlist secret
    "e911a83edce42e894a0461d28e750723f9b7f247b683f636b189d1d4d276d5dd", // pragma: allowlist secret
    "0cec24fa72657e0205200a58899593812e11ad57e44b2a6ed9a99399d297072c", // pragma: allowlist secret
];

/// KNOWN-DEFECT BASELINE: these roots and block hashes embed the replay defect above (note heights 1
/// and 3 share a root). They are byte-identity goldens for the gate-OFF path, changed only by the
/// gated feature.
#[test]
fn known_defect_baseline_roots_and_block_hashes_are_pinned_and_deterministic() {
    let first = peer_chain();
    let second = peer_chain();
    assert_eq!(
        first, second,
        "same fixture, same roots (golden precondition)"
    );
    let roots: Vec<&str> = first.iter().map(|s| s.root.as_str()).collect();
    assert_eq!(
        roots, GOLDEN_PEER_ROOTS,
        "gate-OFF roots must stay byte-identical to main"
    );
    // Genesis (height 0) has no block.
    let hashes: Vec<&str> = first[1..].iter().map(|s| s.block_hash.as_str()).collect();
    assert_eq!(
        hashes, GOLDEN_PEER_BLOCK_HASHES,
        "gate-OFF block hashes must stay byte-identical to main"
    );
}

/// The roots do not see signed transaction or header bytes; the block hashes do. Moving one
/// transaction's timestamp by a second leaves every state root unchanged and changes the block hash,
/// so the hash goldens above are a separate, live check.
#[test]
fn block_hash_goldens_are_sensitive_to_transaction_bytes_that_roots_ignore() {
    install_plain_v1();
    let base = peer_chain();
    let mut state = genesis();
    let moved = apply_peer(
        &mut state,
        &Hash::default(),
        1,
        vec![signed_at(false, 0, "first", 1)],
    );
    assert_eq!(
        state.state_root.to_string(),
        base[1].root,
        "state root ignores the transaction timestamp"
    );
    assert_ne!(
        moved.to_string(),
        base[1].block_hash,
        "block hash covers the transaction bytes the root does not"
    );
}

/// KNOWN-DEFECT BASELINE (producer/applier divergence): the producer emits nothing for a Stake while
/// the peer locks it, so the two roots differ. Preserved gate-OFF; only the gated feature may make
/// producer and peer agree.
#[test]
fn known_defect_baseline_producer_omits_staking_so_its_root_differs_from_the_peer() {
    install_plain_v1();
    let mut producer = genesis();
    let stake = signed(false, 0, "first");
    let transitions = producer.governance_transitions_for_block(&[stake.clone()], 1, 1);
    assert!(
        transitions.is_empty(),
        "main's producer emits no Stake transition (design: governance_transitions_for_block omits staking): {transitions:?}"
    );
    producer
        .apply_transitions_batch_at_height(1, transitions.into_iter())
        .expect("producer applies an empty set");
    producer.apply_block_finalization(1);
    let producer_step = snapshot(&producer, 1);

    let mut peer = genesis();
    apply_peer(&mut peer, &Hash::default(), 1, vec![stake]);
    let peer_step = snapshot(&peer, 1);

    assert_eq!(producer_step.locked, 0, "producer locked nothing");
    assert_eq!(peer_step.locked, 2_000, "peer locked the stake");
    assert_ne!(
        producer_step.root, peer_step.root,
        "producer and peer roots differ today: the divergence the gated feature must remove"
    );
}
