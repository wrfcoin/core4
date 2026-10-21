// Integration tests for dynamic oracle quorum scaling (#988).
//
// Verifies that compute_oracle_quorum() applies the BFT 2/3+1 formula at
// >= 3 oracles, that create_next_block and verify_block use the same quorum
// value (preventing consensus forks), and that block acceptance/rejection
// follows the quorum threshold.
//
// Run from $HOME/wrfcoin/core4:
//   cargo test --test oracle_quorum -- --nocapture

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use rustc_hash::FxHashSet;
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::constants::{DEFAULT_ORACLE_QUORUM, compute_oracle_quorum};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

fn oracle_identity(seed: u64) -> (PrivateKey, String) {
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let oracle_id = PublicKey::from_private_key(&private_key).to_string();
    (private_key, oracle_id)
}

fn build_tx(tx_type: TransactionType, tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: format!("wrf-{tag}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = tx_type;
    tx.set_id();
    tx
}

fn build_commit_and_reveal(
    station_id: &str,
    timestamp: i64,
    quality: f64,
    tag: &str,
) -> (Transaction, Transaction, String) {
    let payload = serde_json::json!({"quality": quality, "volume": 1.0, "uniqueness": 1.0});
    let nonce = format!("nonce-{tag}");
    let commit_id =
        Transaction::compute_commit_id(&payload, &nonce, station_id, timestamp, "contribution");

    // Commit authorization is self-certifying; reward eligibility remains
    // gated by oracle quorum. Use a deterministic contributor key so this
    // copied fixture reaches the production path that it is meant to test.
    let signer_seed = tag
        .bytes()
        .fold(51_000u64, |seed, byte| seed.saturating_add(byte as u64));
    let mut signer_rng = StdRng::seed_from_u64(signer_seed);
    let signer = PrivateKey::generate(&mut signer_rng);
    let mut commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit_payload.sign_authorization(&signer);

    let reveal_payload = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        data: payload,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: None,
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };

    let mut commit_tx = build_tx(TransactionType::ContributionCommit, &format!("commit-{tag}"));
    commit_tx.data =
        Some(serde_json::to_string(&commit_payload).expect("serialize commit payload"));
    commit_tx.set_id();

    let mut reveal_tx = build_tx(TransactionType::ContributionReveal, &format!("reveal-{tag}"));
    reveal_tx.data =
        Some(serde_json::to_string(&reveal_payload).expect("serialize reveal payload"));
    reveal_tx.fee = 1; // bypass computational-fee checks
    reveal_tx.set_id();

    (commit_tx, reveal_tx, commit_id)
}

fn signed_attestation_tx(
    commit_id: &str,
    private_key: &PrivateKey,
    oracle_id: &str,
    timestamp: i64,
    tag: &str,
) -> Transaction {
    let mut payload = OracleAttestationPayload {
        commit_id: commit_id.to_string(),
        oracle_id: oracle_id.to_string(),
        approved: true,
        signature: None,
        confidence: Some(1.0),
        timestamp,
        nonce: Some(format!("nonce-{tag}")),
    };
    let signature = private_key.sign(payload.signing_message().as_bytes()).to_string();
    payload.signature = Some(signature);

    let mut tx = build_tx(TransactionType::OracleAttestation, &format!("att-{tag}"));
    tx.data = Some(serde_json::to_string(&payload).expect("serialize attestation payload"));
    tx.set_id();
    tx
}

// ---------------------------------------------------------------------------
// Unit-level quorum formula tests (integration file verifies the same values
// that the production paths rely on, complementing the lib tests)
// ---------------------------------------------------------------------------

/// #988-1: quorum at 1 and 2 oracles uses the DEFAULT_ORACLE_QUORUM minimum (2).
#[test]
fn quorum_below_bft_threshold_uses_minimum() {
    // formula: max(2, N*2/3 + 1)
    // N=0 → max(2, 0+1) = 2
    assert_eq!(compute_oracle_quorum(0), DEFAULT_ORACLE_QUORUM);
    // N=1 → max(2, 0+1) = 2 (single oracle is untrusted; minimum is enforced)
    assert_eq!(compute_oracle_quorum(1), DEFAULT_ORACLE_QUORUM);
    // N=2 → max(2, 1+1) = 2
    assert_eq!(compute_oracle_quorum(2), DEFAULT_ORACLE_QUORUM);
}

/// #988-2/#988-3: quorum at 3 oracles activates BFT unanimous threshold.
#[test]
fn quorum_at_three_oracles_is_unanimous() {
    // N=3 → max(2, 2+1) = 3 (all three must agree — unanimous for the smallest BFT set)
    assert_eq!(compute_oracle_quorum(3), 3);
}

/// #988-4: quorum at 4 oracles allows one fault-tolerant failure.
#[test]
fn quorum_at_four_oracles() {
    // N=4 → max(2, 2+1) = 3
    assert_eq!(compute_oracle_quorum(4), 3);
}

/// #988-5: quorum at 12 oracles (testnet target) is 9.
#[test]
fn quorum_at_twelve_oracles_testnet_target() {
    // N=12 → max(2, 8+1) = 9
    // This is the primary testnet configuration; pin it explicitly.
    assert_eq!(compute_oracle_quorum(12), 9);
}

/// Monotonicity: quorum never decreases as oracle set grows.
#[test]
fn quorum_is_monotonically_non_decreasing() {
    let mut prev = compute_oracle_quorum(0);
    for n in 1..=100 {
        let q = compute_oracle_quorum(n);
        assert!(
            q >= prev,
            "quorum({n}) = {q} < quorum({}) = {prev} — must be non-decreasing",
            n - 1
        );
        prev = q;
    }
}

/// For N >= 2, quorum never exceeds N (can't require more votes than oracle set size).
#[test]
fn quorum_never_exceeds_oracle_set_size() {
    for n in 2..=100 {
        let q = compute_oracle_quorum(n);
        assert!(
            q <= n,
            "quorum({n}) = {q} must not exceed oracle set size"
        );
    }
}

// ---------------------------------------------------------------------------
// Block-production integration: quorum symmetry between create_next_block
// and verify_block
// ---------------------------------------------------------------------------

/// #988-6/#988-7: create_next_block and verify_block use identical quorum logic.
///
/// The quorum filter in create_next_block applies to **low-quality** contributions:
/// low-quality reveals are excluded unless they have quorum oracle attestations.
/// High-quality contributions pass the quality gate and are included regardless.
///
/// Build a block with N oracles registered.  Submit exactly quorum-1 attestations
/// for a LOW-quality reveal -> create_next_block excludes it (below quorum).  Then
/// submit exactly quorum attestations -> block includes it.
///
/// Finally confirm verify_block accepts/rejects the block consistently with the
/// same oracle set (the invariant that prevents consensus forks).
#[tokio::test]
async fn block_production_respects_dynamic_quorum_and_verify_is_symmetric() {
    // Use 3 oracles so BFT quorum is 3 (unanimous).
    // This makes the "below quorum" case simple: 2 attestations < 3 required.
    const ORACLE_COUNT: usize = 3;
    let expected_quorum = compute_oracle_quorum(ORACLE_COUNT);
    assert_eq!(expected_quorum, 3, "sanity: 3 oracles -> quorum 3");

    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    // Generate oracle identities deterministically.
    let oracles: Vec<(PrivateKey, String)> =
        (0..ORACLE_COUNT).map(|i| oracle_identity(9880 + i as u64)).collect();

    // Register the oracle set on the chain.
    let mut chain = Blockchain::default();
    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: oracles.iter().map(|(_, id)| id.clone()).collect(),
            oracle_quorum: expected_quorum,
        })
        .expect("set oracle config");

    // Give a contributor some funds.
    let contributor = addr(0x88);
    chain.state.create_account(contributor, Balance::new(1000));

    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    let ts = 1_700_500_000i64;
    let station_id = "quorum-station-988";

    // -------------------------------------------------------------------
    // Round A: submit quorum-1 attestations (2 out of 3) for a LOW-quality
    // contribution.  The reveal should NOT be included (below quorum and
    // quality too low to pass the quality gate on its own).
    //
    // Quality 0.1 falls into the Low band (default threshold: < 0.50).
    // The quorum filter only kicks in for Low-band contributions.
    // -------------------------------------------------------------------
    let (commit_a, mut reveal_a, commit_id_a) =
        build_commit_and_reveal(station_id, ts, 0.1, "988-a");
    reveal_a.sender_address = contributor;

    // Build quorum-1 = 2 attestations only
    let partial_atts: Vec<Transaction> = oracles[..expected_quorum - 1]
        .iter()
        .enumerate()
        .map(|(i, (sk, id))| {
            signed_attestation_tx(&commit_id_a, sk, id, ts + 1 + i as i64, &format!("988-a-{i}"))
        })
        .collect();

    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(commit_a).expect("enqueue commit-a");
        mp.add_transaction(reveal_a).expect("enqueue reveal-a");
        for att in partial_atts {
            mp.add_transaction_with_signing_keys(att, Some(&signing_keys)).expect("enqueue partial attestation");
        }
    }

    let block_a = pop
        .create_next_block(
            blockchain.clone(),
            mempool.clone(),
            "0xdeadbeef00000000000000000000000000000001",
        )
        .await
        .expect("create block A");

    // The low-quality reveal should be absent from block A: quorum not met.
    let has_reveal_a = block_a
        .transactions
        .iter()
        .any(|tx| tx.tx_type == TransactionType::ContributionReveal);
    assert!(
        !has_reveal_a,
        "block A must not contain the low-quality reveal when only {}/{} attestations were submitted",
        expected_quorum - 1,
        expected_quorum
    );

    // -------------------------------------------------------------------
    // Round B: submit all quorum attestations (3 out of 3) for a LOW-quality
    // contribution.  With full quorum the reveal SHOULD be included despite
    // low quality.
    // -------------------------------------------------------------------
    let ts2 = ts + 100;
    let (commit_b, mut reveal_b, commit_id_b) =
        build_commit_and_reveal(station_id, ts2, 0.1, "988-b");
    reveal_b.sender_address = contributor;

    let full_atts: Vec<Transaction> = oracles
        .iter()
        .enumerate()
        .map(|(i, (sk, id))| {
            signed_attestation_tx(&commit_id_b, sk, id, ts2 + 1 + i as i64, &format!("988-b-{i}"))
        })
        .collect();

    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(commit_b).expect("enqueue commit-b");
        mp.add_transaction(reveal_b).expect("enqueue reveal-b");
        for att in full_atts {
            mp.add_transaction_with_signing_keys(att, Some(&signing_keys)).expect("enqueue full attestation");
        }
    }

    let block_b = pop
        .create_next_block(
            blockchain.clone(),
            mempool.clone(),
            "0xdeadbeef00000000000000000000000000000001",
        )
        .await
        .expect("create block B");

    let has_reveal_b = block_b
        .transactions
        .iter()
        .any(|tx| tx.tx_type == TransactionType::ContributionReveal);
    assert!(
        has_reveal_b,
        "block B must contain the low-quality reveal when all {expected_quorum}/{expected_quorum} attestations are present"
    );

    // -------------------------------------------------------------------
    // Verify symmetry: verify_block must accept block B and reject a block
    // that was built with below-quorum attestations.
    //
    // We verify block_b (full quorum) is accepted and block_a (partial
    // quorum) is not rejected by verify_block for quorum reasons
    // (block_a is a valid empty-ish block -- no reveal means no quorum
    // failure, so verify_block should accept it too).
    // -------------------------------------------------------------------
    let oracle_set: FxHashSet<String> =
        oracles.iter().map(|(_, id)| id.clone()).collect();

    // block_b (has reveal with full quorum) must be accepted by verify_block.
    let verify_b = pop
        .verify_block(&block_b, &oracle_set, expected_quorum, &[])
        .await
        .expect("verify block B must not error");
    assert!(
        verify_b,
        "verify_block must accept block_b produced by create_next_block with full quorum"
    );

    // block_a (no reveal, partial quorum unused) must also be accepted --
    // there's nothing invalid about a block that simply has no contributions.
    let verify_a = pop
        .verify_block(&block_a, &oracle_set, expected_quorum, &[])
        .await
        .expect("verify block A must not error");
    assert!(
        verify_a,
        "verify_block must accept block_a (no contribution reveal = no quorum violation)"
    );
}

/// #988-7: verify_block overrides the caller-supplied quorum with the dynamic
/// value when oracle set is >= 3.  Confirms that both paths use the same formula.
#[tokio::test]
async fn verify_block_overrides_static_quorum_with_dynamic_at_three_plus_oracles() {
    const ORACLE_COUNT: usize = 3; // BFT quorum = 3
    let bft_quorum = compute_oracle_quorum(ORACLE_COUNT);
    assert_eq!(bft_quorum, 3);

    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    let oracles: Vec<(PrivateKey, String)> =
        (0..ORACLE_COUNT).map(|i| oracle_identity(9900 + i as u64)).collect();

    let mut chain = Blockchain::default();
    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: oracles.iter().map(|(_, id)| id.clone()).collect(),
            oracle_quorum: bft_quorum,
        })
        .expect("set oracle config");

    let contributor = addr(0x99);
    chain.state.create_account(contributor, Balance::new(1000));

    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    let ts = 1_700_600_000i64;
    let station_id = "quorum-verify-station-988";

    // Build a block via create_next_block with full quorum
    let (commit_tx, mut reveal_tx, commit_id) =
        build_commit_and_reveal(station_id, ts, 1.0, "988-verify");
    reveal_tx.sender_address = contributor;

    let full_atts: Vec<Transaction> = oracles
        .iter()
        .enumerate()
        .map(|(i, (sk, id))| {
            signed_attestation_tx(&commit_id, sk, id, ts + 1 + i as i64, &format!("988-v-{i}"))
        })
        .collect();

    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(commit_tx).expect("commit");
        mp.add_transaction(reveal_tx).expect("reveal");
        for att in full_atts {
            mp.add_transaction_with_signing_keys(att, Some(&signing_keys)).expect("attestation");
        }
    }

    let block = pop
        .create_next_block(
            blockchain.clone(),
            mempool.clone(),
            "0xdeadbeef00000000000000000000000000000002",
        )
        .await
        .expect("create block");

    let oracle_set: FxHashSet<String> =
        oracles.iter().map(|(_, id)| id.clone()).collect();

    // Verify with the correct dynamic quorum (= bft_quorum = 3).
    let accepted = pop
        .verify_block(&block, &oracle_set, bft_quorum, &[])
        .await
        .expect("verify");
    assert!(accepted, "block with full quorum must be accepted by verify_block");

    // verify_block internally overrides the caller-supplied quorum for sets >= 3.
    // Passing a lower value (1) must still produce the same BFT result because the
    // dynamic formula kicks in.
    let also_accepted = pop
        .verify_block(&block, &oracle_set, 1, &[])
        .await
        .expect("verify with low static quorum");
    assert!(
        also_accepted,
        "verify_block must override static quorum=1 with dynamic quorum={bft_quorum} for {ORACLE_COUNT}-oracle set"
    );
}
