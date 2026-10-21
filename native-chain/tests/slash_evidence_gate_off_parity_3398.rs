//! #3398 S1 gate-OFF parity against main's existing pinned staking fixture.
//! Fixture builders and unchanged goldens ported from stake_gate_off_baseline_3430.
//! This separate target avoids editing the parked staking review's fixture.

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
        assert_off_report_submission_leaves_bytes_unchanged(&mut state, height);
        prev = apply_peer(&mut state, &prev, height, txs);
        let mut step = snapshot(&state, height);
        step.block_hash = prev.to_string();
        steps.push(step);
    }
    steps
}

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

#[test]
fn slash_evidence_gate_off_preserves_main_fixture_roots_and_blocks() {
    use wrfcoin_shared_protocol::crypto::chain_binding::{ConsensusFeature, feature_active};
    let steps = peer_chain();
    for step in &steps {
        assert!(!feature_active(
            ConsensusFeature::SlashEvidenceReports,
            step.height
        ));
    }
    assert!(!feature_active(
        ConsensusFeature::SlashEvidenceReports,
        u64::MAX
    ));
    assert_eq!(
        steps.iter().map(|s| s.root.as_str()).collect::<Vec<_>>(),
        GOLDEN_PEER_ROOTS
    );
    assert_eq!(
        steps[1..]
            .iter()
            .map(|s| s.block_hash.as_str())
            .collect::<Vec<_>>(),
        GOLDEN_PEER_BLOCK_HASHES
    );
}

#[path = "../../shared-protocol/tests/common/slash_evidence_vectors.rs"]
mod report_vectors;

fn assert_off_report_submission_leaves_bytes_unchanged(state: &mut State, height: u64) {
    use wrfcoin_shared_protocol::blockchain::state::StateTransition;
    use wrfcoin_shared_protocol::config::settings::Config;
    use wrfcoin_shared_protocol::mempool::pool::Mempool;
    let mut payload = report_vectors::payload();
    payload
        .sign_for(&report_vectors::reporter(), &report_vectors::identity())
        .unwrap();
    let mut tx = Transaction::new_coinbase("synthetic-report", 0, height);
    tx.tx_type = TransactionType::SlashEvidenceReport;
    tx.inputs[0].coinbase_data = None;
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.timestamp = fixture_time(height as i64);
    tx.set_id();
    let serialized = bincode::serialize(&*state).unwrap();
    let root = state.state_root_material();
    let mut pool = Mempool::new(&Config::default());
    assert!(
        pool.add_transaction(tx.clone())
            .unwrap_err()
            .to_string()
            .contains("SP_SLASH_EVIDENCE_NOT_SUPPORTED")
    );
    assert_eq!(pool.size(), 0);
    let error = state
        .apply_transition(StateTransition::ProcessGovernanceTransaction {
            tx: tx.clone(),
            current_block: height,
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("SP_SLASH_EVIDENCE_NOT_SUPPORTED"), "{error}");
    assert_eq!(
        bincode::serialize(&*state).unwrap(),
        serialized,
        "report submission changed serialized state at {height}"
    );
    assert_eq!(
        state.state_root_material(),
        root,
        "report submission changed root bytes at {height}"
    );
    // The peer extractor must also omit the refused report entirely.
    let block = Block::new(1, Hash::default(), vec![tx], 1, height);
    assert!(extract_transitions(state, &block, height, block.timestamp.timestamp()).is_empty());
}
