use std::collections::HashMap;
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use tokio::task::JoinSet;
use tokio::time::{Duration, sleep};
use wrfcoin_shared_protocol::blockchain::transaction::{Operation, Transaction};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::layer2::channel::ChannelStateUpdate;
use wrfcoin_shared_protocol::layer2::proof::{
    ProofSystem, ZKGenerator, ZKProof, ZKVerifier, derive_proof_digest,
};
use wrfcoin_shared_protocol::layer2::rollup::Rollup;
use wrfcoin_shared_protocol::layer2::state::{
    Layer2Account, Layer2State, StateProof, StateProofType, StateTransition,
};
use wrfcoin_shared_protocol::layer2::verifier::RollupVerifier;
use wrfcoin_shared_protocol::layer2::{
    BatchStatus, ChannelStatus, ChannelType, DataStorageMode, L1BatchReceipt, L1BatchReceiptEvent,
    OperatorConfig, OperatorStatus, RollupConfig, RollupError, RollupOperator, RollupType,
    StateChannel, VerificationMode, VerifierConfig,
};

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

fn vk_from_seed(byte: u8) -> PublicKey {
    let sk = PrivateKey::from_bytes(&[byte; 32]).expect("seed private key");
    PublicKey::from_private_key(&sk)
}

fn hash(byte: u8) -> Hash {
    Hash::from_bytes([byte; 32])
}

fn fixed_timestamp(offset_secs: i64) -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_710_000_000 + offset_secs, 0)
        .single()
        .expect("valid fixed timestamp")
}

fn transfer_tx(
    sender: Address,
    recipient: Address,
    amount: u64,
    fee: u64,
    nonce: u64,
    offset_secs: i64,
) -> Transaction {
    let mut tx = Transaction::new(vec![], vec![]);
    tx.sender_address = sender;
    tx.recipient_address = Some(recipient);
    tx.amount = amount;
    tx.fee = fee;
    tx.nonce = nonce;
    tx.operation = Operation::Transfer { amount };
    tx.timestamp = fixed_timestamp(offset_secs);
    tx.set_id();
    tx
}

fn self_transfer_tx(
    sender: Address,
    amount: u64,
    fee: u64,
    nonce: u64,
    offset_secs: i64,
) -> Transaction {
    let mut tx = transfer_tx(sender, sender, amount, fee, nonce, offset_secs);
    tx.recipient_address = Some(sender);
    tx.set_id();
    tx
}

fn public_inputs(from_state: Hash, to_state: Hash, extra: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(64 + extra.len());
    bytes.extend_from_slice(from_state.as_bytes());
    bytes.extend_from_slice(to_state.as_bytes());
    bytes.extend_from_slice(extra);
    bytes
}

async fn generated_proof(
    proof_system: ProofSystem,
    circuit_id: &str,
    from_state: Hash,
    to_state: Hash,
    extra_public: &[u8],
    private_inputs: Vec<u8>,
) -> ZKProof {
    let mut generator = ZKGenerator::new(proof_system);
    generator
        .generate_proof(
            circuit_id,
            public_inputs(from_state, to_state, extra_public),
            private_inputs,
        )
        .await
        .expect("proof generation should succeed")
}

fn zk_rollup_config(id: &str) -> RollupConfig {
    RollupConfig {
        id: id.to_string(),
        name: format!("Rollup {id}"),
        rollup_type: RollupType::ZeroKnowledge,
        operator_address: addr(0x90),
        batch_size: 2,
        max_batches_per_block: 4,
        challenge_period_blocks: 7,
        l1_gas_limit_per_batch: 1_000_000,
        verification_contract_address: Some(addr(0x91)),
        data_availability_committee: Some(vec![addr(0x92), addr(0x93)]),
        data_storage_mode: DataStorageMode::OnChain,
        fee_token_address: Some(addr(0x94)),
    }
}

fn optimistic_rollup_config(id: &str) -> RollupConfig {
    RollupConfig {
        rollup_type: RollupType::Optimistic,
        challenge_period_blocks: 10,
        ..zk_rollup_config(id)
    }
}

async fn ready_empty_batch_rollup(id: &str) -> (Rollup, u64) {
    let rollup = Rollup::new(zk_rollup_config(id))
        .await
        .expect("rollup init");
    let batch_id = rollup.start_new_batch().await.expect("start batch");
    rollup.finalize_batch().await.expect("finalize empty batch");
    (rollup, batch_id)
}

async fn active_verifier(mode: VerificationMode) -> RollupVerifier {
    let config = VerifierConfig {
        mode,
        auto_verify: false,
        verification_interval: 1,
        max_batch_size: 16,
        verification_timeout: 10,
        concurrent_verifications: 2,
        ..VerifierConfig::default()
    };
    let mut verifier = RollupVerifier::new(config, Layer2State::new());
    verifier.start().await.expect("start verifier");
    verifier
}

fn funded_state(pairs: &[(Address, u64)]) -> Layer2State {
    let mut state = Layer2State::new();
    for (address, balance) in pairs {
        state.accounts.insert(
            *address,
            Layer2Account {
                address: *address,
                balance: *balance,
                nonce: 0,
                storage: HashMap::new(),
                code: None,
                last_update: 0,
            },
        );
    }
    state
}

macro_rules! proof_system_roundtrip_test {
    ($name:ident, $system:expr, $seed:expr) => {
        #[tokio::test]
        async fn $name() {
            let seed: u8 = $seed;
            let from_state = hash(seed);
            let to_state = hash(seed.wrapping_add(1));
            let extra_public = vec![seed; 48];
            let private_inputs = vec![seed.wrapping_add(2); 64];
            let proof = generated_proof(
                $system,
                stringify!($name),
                from_state,
                to_state,
                &extra_public,
                private_inputs,
            )
            .await;

            let verifier = ZKVerifier::new();
            assert!(
                verifier
                    .verify(&proof, from_state, to_state)
                    .await
                    .expect("verification succeeds"),
                "generated proof must verify"
            );

            let mut tampered = proof.clone();
            tampered.proof_data[0] ^= 0xff;
            assert!(
                !verifier
                    .verify(&tampered, from_state, to_state)
                    .await
                    .expect("tampered verification returns false"),
                "tampered proof bytes must fail verification"
            );
        }
    };
}

// Proof <-> verifier round-trip

proof_system_roundtrip_test!(
    l2_state_update_proof_verify_roundtrip_groth16,
    ProofSystem::Groth16,
    0x11
);
proof_system_roundtrip_test!(
    l2_state_update_proof_verify_roundtrip_plonk,
    ProofSystem::Plonk,
    0x22
);
proof_system_roundtrip_test!(
    l2_state_update_proof_verify_roundtrip_bulletproofs,
    ProofSystem::Bulletproofs,
    0x33
);
proof_system_roundtrip_test!(
    l2_state_update_proof_verify_roundtrip_halo2,
    ProofSystem::Halo2,
    0x44
);
proof_system_roundtrip_test!(
    l2_state_update_proof_verify_roundtrip_custom,
    ProofSystem::Custom,
    0x55
);

#[tokio::test]
async fn proof_generator_is_deterministic_for_identical_inputs() {
    let from_state = hash(0x61);
    let to_state = hash(0x62);
    let extra_public = vec![0x63; 96];
    let private_inputs = vec![0x64; 128];

    let first = generated_proof(
        ProofSystem::Groth16,
        "deterministic-proof",
        from_state,
        to_state,
        &extra_public,
        private_inputs.clone(),
    )
    .await;
    let second = generated_proof(
        ProofSystem::Groth16,
        "deterministic-proof",
        from_state,
        to_state,
        &extra_public,
        private_inputs,
    )
    .await;

    assert_eq!(
        first.to_bytes(),
        second.to_bytes(),
        "same inputs must produce identical proof bytes"
    );
}

#[tokio::test]
async fn proof_large_batch_verifies_with_stable_commitment() {
    let from_state = hash(0x71);
    let to_state = hash(0x72);
    let updates: Vec<Hash> = (0..128).map(|i| hash((i as u8).wrapping_add(1))).collect();
    let mut extra_public = Vec::with_capacity(updates.len() * 32);
    for update in &updates {
        extra_public.extend_from_slice(update.as_bytes());
    }

    let proof = generated_proof(
        ProofSystem::Halo2,
        "large-batch",
        from_state,
        to_state,
        &extra_public,
        extra_public.clone(),
    )
    .await;

    let verifier = ZKVerifier::new();
    assert!(
        verifier
            .verify(&proof, from_state, to_state)
            .await
            .expect("verification succeeds"),
        "batched proof must verify"
    );
}

#[test]
fn proof_serialization_roundtrip_survives_binary_roundtrip() {
    let proof = ZKProof::new(vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]);
    let encoded = proof.to_bytes();
    let decoded = ZKProof::from_bytes(&encoded).expect("decode proof");
    let bincode_bytes = bincode::serialize(&decoded).expect("bincode encode proof");
    let decoded_again: ZKProof = bincode::deserialize(&bincode_bytes).expect("bincode decode");

    assert_eq!(decoded_again.to_bytes(), encoded);
}

#[test]
fn proof_from_bytes_rejects_truncated_fields() {
    let proof = ZKProof::new(vec![1, 2, 3, 4], vec![5, 6], vec![7, 8, 9, 10]);
    let encoded = proof.to_bytes();

    assert!(ZKProof::from_bytes(&encoded[..4]).is_err());
    assert!(ZKProof::from_bytes(&encoded[..encoded.len() - 2]).is_err());
}

#[tokio::test]
async fn verifier_rejects_wrong_state_roots_even_when_digest_matches() {
    let from_state = hash(0x81);
    let to_state = hash(0x82);
    let wrong_from = hash(0x83);
    let proof = generated_proof(
        ProofSystem::Plonk,
        "wrong-roots",
        from_state,
        to_state,
        b"fixture",
        vec![9; 32],
    )
    .await;

    let verifier = ZKVerifier::new();
    assert!(
        !verifier
            .verify(&proof, wrong_from, to_state)
            .await
            .expect("verification completes"),
        "mismatched roots must be rejected"
    );
}

#[test]
fn derive_proof_digest_is_stable_for_pinned_fixture() {
    let digest = derive_proof_digest(b"fixture-public-inputs", b"fixture-verification-key");
    let expected = [
        235, 16, 35, 119, 32, 67, 192, 22, 50, 165, 47, 117, 138, 139, 130, 78, 202, 115, 203, 204,
        161, 60, 141, 44, 213, 227, 51, 154, 30, 142, 201, 30,
    ];
    assert_eq!(digest, expected, "pinned digest must remain stable");
}

// Rollup submission pipeline

#[tokio::test]
async fn rollup_submit_verify_settle_e2e_empty_batch() {
    let (rollup, batch_id) = ready_empty_batch_rollup("e2e-empty").await;
    let submitted_hash = rollup.submit_batch(batch_id).await.expect("submit batch");
    assert!(
        rollup.verify_batch(batch_id).await.expect("verify batch"),
        "ready zero-knowledge batch must verify"
    );

    rollup
        .ingest_l1_receipt(L1BatchReceipt {
            batch_id,
            l1_transaction_hash: submitted_hash,
            event: L1BatchReceiptEvent::InclusionObserved {
                l1_block_number: 200,
                l1_block_timestamp: 1_700_000_010,
            },
        })
        .await
        .expect("record inclusion");
    rollup
        .ingest_l1_receipt(L1BatchReceipt {
            batch_id,
            l1_transaction_hash: submitted_hash,
            event: L1BatchReceiptEvent::FinalityObserved {
                observed_l1_block: 205,
                observed_timestamp: 1_700_000_020,
            },
        })
        .await
        .expect("record finality");

    let batch = rollup.get_batch(batch_id).await.expect("batch exists");
    assert_eq!(batch.status, BatchStatus::Finalized);
    assert_eq!(batch.l1_finality_block_number, Some(205));
}

#[tokio::test]
async fn rollup_add_transaction_requires_open_batch() {
    let rollup = Rollup::new(zk_rollup_config("no-batch"))
        .await
        .expect("rollup init");
    let tx = transfer_tx(addr(1), addr(2), 5, 1, 0, 1);
    let err = rollup.add_transaction(tx).await.expect_err("must reject");

    assert!(err.to_string().contains("No batch in progress"));
    assert_eq!(rollup.get_pending_transactions().await.len(), 1);
}

#[tokio::test]
async fn rollup_add_transaction_rejects_missing_recipient() {
    let rollup = Rollup::new(zk_rollup_config("missing-recipient"))
        .await
        .expect("rollup init");
    rollup.start_new_batch().await.expect("start batch");

    let mut tx = Transaction::new(vec![], vec![]);
    tx.sender_address = addr(3);
    tx.amount = 5;
    tx.fee = 1;
    tx.nonce = 0;
    tx.recipient_address = None;
    tx.set_id();

    let err = rollup.add_transaction(tx).await.expect_err("must reject");
    assert!(err.to_string().contains("recipient_address"));
}

#[tokio::test]
async fn rollup_add_transaction_rejects_zero_value_and_zero_fee() {
    let rollup = Rollup::new(zk_rollup_config("zero-value"))
        .await
        .expect("rollup init");
    rollup.start_new_batch().await.expect("start batch");

    let tx = transfer_tx(addr(4), addr(5), 0, 0, 0, 2);
    let err = rollup.add_transaction(tx).await.expect_err("must reject");
    assert!(err.to_string().contains("transfer value or pay fee"));
}

#[tokio::test]
async fn rollup_add_transaction_rejects_duplicate_transaction_hash() {
    let rollup = Rollup::new(zk_rollup_config("duplicate-hash"))
        .await
        .expect("rollup init");
    rollup.start_new_batch().await.expect("start batch");

    let first = transfer_tx(addr(6), addr(7), 10, 1, 0, 3);
    let mut duplicate = transfer_tx(addr(6), addr(8), 10, 1, 1, 4);
    duplicate.id = first.id;

    rollup
        .add_transaction(first)
        .await
        .expect("first transaction accepted");
    let err = rollup
        .add_transaction(duplicate)
        .await
        .expect_err("duplicate hash must reject");
    assert!(err.to_string().contains("Duplicate transaction hash"));
}

#[tokio::test]
async fn rollup_add_transaction_rejects_duplicate_sender_nonce_pair() {
    let rollup = Rollup::new(zk_rollup_config("duplicate-nonce"))
        .await
        .expect("rollup init");
    rollup.start_new_batch().await.expect("start batch");

    let first = transfer_tx(addr(9), addr(10), 10, 1, 0, 5);
    let second = transfer_tx(addr(9), addr(11), 12, 1, 0, 6);

    rollup
        .add_transaction(first)
        .await
        .expect("first transaction accepted");
    let err = rollup
        .add_transaction(second)
        .await
        .expect_err("duplicate sender/nonce must reject");
    assert!(err.to_string().contains("Duplicate sender/nonce"));
}

#[tokio::test]
async fn rollup_add_transaction_rejects_self_transfer() {
    let rollup = Rollup::new(zk_rollup_config("self-transfer"))
        .await
        .expect("rollup init");
    rollup.start_new_batch().await.expect("start batch");

    let tx = self_transfer_tx(addr(12), 10, 1, 0, 7);
    let err = rollup.add_transaction(tx).await.expect_err("must reject");
    assert!(
        err.to_string()
            .contains("Sender and recipient must be different")
    );
}

#[tokio::test]
async fn rollup_current_batch_full_moves_transaction_to_pending() {
    let config = RollupConfig {
        batch_size: 1,
        ..zk_rollup_config("batch-full")
    };
    let rollup = Rollup::new(config).await.expect("rollup init");
    rollup.start_new_batch().await.expect("start batch");

    let first = transfer_tx(addr(13), addr(14), 10, 1, 0, 8);
    let second = transfer_tx(addr(15), addr(16), 11, 1, 0, 9);

    rollup
        .add_transaction(first)
        .await
        .expect("first transaction accepted");
    let err = rollup
        .add_transaction(second.clone())
        .await
        .expect_err("full batch must reject additional tx");
    assert!(err.to_string().contains("Current batch is full"));

    let pending = rollup.get_pending_transactions().await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, second.id);
}

#[tokio::test]
async fn rollup_submit_batch_rejects_non_ready_batches() {
    let rollup = Rollup::new(zk_rollup_config("submit-before-ready"))
        .await
        .expect("rollup init");
    let batch_id = rollup.start_new_batch().await.expect("start batch");

    let err = rollup
        .submit_batch(batch_id)
        .await
        .expect_err("non-ready batch must reject");
    assert!(err.to_string().contains("Batch not found"));
}

#[tokio::test]
async fn rollup_finalization_requires_l1_inclusion_and_confirmations() {
    let (rollup, batch_id) = ready_empty_batch_rollup("finality-guards").await;
    rollup.submit_batch(batch_id).await.expect("submit batch");

    let err = rollup
        .finalize_submitted_batch(batch_id, 100, 1_700_000_000)
        .await
        .expect_err("missing inclusion must reject");
    assert!(err.to_string().contains("inclusion block is missing"));

    rollup
        .record_l1_inclusion(batch_id, 100, 1_700_000_010)
        .await
        .expect("record inclusion");
    let err = rollup
        .finalize_submitted_batch(batch_id, 104, 1_700_000_020)
        .await
        .expect_err("insufficient confirmations must reject");
    assert!(err.to_string().contains("Insufficient confirmations"));
}

#[tokio::test]
async fn rollup_ingest_receipt_rejects_hash_mismatch() {
    let (rollup, batch_id) = ready_empty_batch_rollup("receipt-hash-mismatch").await;
    let _submitted = rollup.submit_batch(batch_id).await.expect("submit batch");

    let err = rollup
        .ingest_l1_receipt(L1BatchReceipt {
            batch_id,
            l1_transaction_hash: hash(0xde),
            event: L1BatchReceiptEvent::InclusionObserved {
                l1_block_number: 10,
                l1_block_timestamp: 20,
            },
        })
        .await
        .expect_err("mismatched receipt must reject");
    assert!(err.to_string().contains("hash mismatch"));
}

#[tokio::test]
async fn optimistic_rollup_challenge_flow_changes_status() {
    let rollup = Rollup::new(optimistic_rollup_config("optimistic-challenge"))
        .await
        .expect("rollup init");
    let batch_id = rollup.start_new_batch().await.expect("start batch");
    rollup.finalize_batch().await.expect("finalize batch");
    rollup.submit_batch(batch_id).await.expect("submit batch");
    rollup
        .record_l1_inclusion(batch_id, 300, 1_700_000_000)
        .await
        .expect("record inclusion");
    rollup
        .challenge_submitted_batch(batch_id, "fraud proof")
        .await
        .expect("challenge");

    let batch = rollup.get_batch(batch_id).await.expect("batch exists");
    assert_eq!(batch.status, BatchStatus::Challenged);
    assert!(
        rollup
            .finalize_submitted_batch(batch_id, 320, 1_700_000_100)
            .await
            .is_err(),
        "challenged batches must not finalize"
    );
}

#[tokio::test]
async fn rollup_non_empty_batch_with_seeded_balances() {
    let rollup = Rollup::new(zk_rollup_config("requires-funding"))
        .await
        .expect("rollup init");
    rollup
        .fund_account(addr(20), 25)
        .await
        .expect("seed sender balance");
    rollup.start_new_batch().await.expect("start batch");

    let tx = transfer_tx(addr(20), addr(21), 10, 1, 0, 10);
    rollup.add_transaction(tx).await.expect("validation passes");

    let batch_id = rollup
        .finalize_batch()
        .await
        .expect("seeded state should allow finalization");
    let batch = rollup.get_batch(batch_id).await.expect("batch exists");
    assert_eq!(batch.status, BatchStatus::ReadyForSubmission);
    assert_eq!(batch.transactions.len(), 1);
}

#[tokio::test]
async fn fund_account_after_start_new_batch_errors() {
    let rollup = Rollup::new(zk_rollup_config("funding-guard"))
        .await
        .expect("rollup init");
    rollup.start_new_batch().await.expect("start batch");

    let err = rollup
        .fund_account(addr(22), 100)
        .await
        .expect_err("funding after first batch start must reject");
    let rollup_err = err
        .downcast_ref::<RollupError>()
        .expect("rollup error type should be preserved");
    assert!(matches!(rollup_err, RollupError::FundingAfterBatchStart));
}

#[tokio::test]
async fn rollup_submission_hash_is_deterministic_across_identical_empty_batches() {
    let (first, first_batch) = ready_empty_batch_rollup("deterministic-submit").await;
    let (second, second_batch) = ready_empty_batch_rollup("deterministic-submit").await;

    let first_hash = first.submit_batch(first_batch).await.expect("submit first");
    let second_hash = second
        .submit_batch(second_batch)
        .await
        .expect("submit second");
    assert_eq!(first_hash, second_hash);
}

// Operator lifecycle

#[tokio::test]
async fn operator_new_starts_in_initializing_status() {
    let rollup = Arc::new(
        Rollup::new(zk_rollup_config("operator-new"))
            .await
            .expect("rollup init"),
    );
    let operator = RollupOperator::new(OperatorConfig::default(), rollup);
    assert_eq!(operator.get_status().await, OperatorStatus::Initializing);
}

#[tokio::test]
async fn operator_start_and_stop_transition_status() {
    let rollup = Arc::new(
        Rollup::new(zk_rollup_config("operator-status"))
            .await
            .expect("rollup init"),
    );
    let mut operator = RollupOperator::new(
        OperatorConfig {
            batch_interval: 1,
            submission_interval: 1,
            ..OperatorConfig::default()
        },
        rollup,
    );

    operator.start().await.expect("start operator");
    tokio::task::yield_now().await;
    assert_eq!(operator.get_status().await, OperatorStatus::Active);

    operator.stop().await.expect("stop operator");
    sleep(Duration::from_millis(1_200)).await;
    tokio::task::yield_now().await;
    assert_eq!(operator.get_status().await, OperatorStatus::Paused);
}

#[tokio::test]
async fn operator_stop_without_start_is_rejected() {
    let rollup = Arc::new(
        Rollup::new(zk_rollup_config("operator-stop-guard"))
            .await
            .expect("rollup init"),
    );
    let operator = RollupOperator::new(OperatorConfig::default(), rollup);
    let err = operator.stop().await.expect_err("must reject");
    assert!(err.to_string().contains("Operator not started"));
}

#[tokio::test]
async fn operator_submit_transaction_rejects_when_inactive() {
    let rollup = Arc::new(
        Rollup::new(zk_rollup_config("operator-inactive-submit"))
            .await
            .expect("rollup init"),
    );
    let operator = RollupOperator::new(OperatorConfig::default(), rollup);
    let tx = transfer_tx(addr(30), addr(31), 10, 1, 0, 11);

    let err = operator
        .submit_transaction(tx)
        .await
        .expect_err("inactive operator must reject");
    assert!(err.to_string().contains("Operator is not active"));
}

#[tokio::test]
async fn operator_get_rollup_state_matches_underlying_state() {
    let rollup = Arc::new(
        Rollup::new(zk_rollup_config("operator-state"))
            .await
            .expect("rollup init"),
    );
    let operator = RollupOperator::new(OperatorConfig::default(), rollup.clone());

    let state = operator.get_rollup_state().await.expect("get state");
    assert_eq!(
        state.get_state_root(),
        rollup.get_state().await.get_state_root()
    );
}

#[tokio::test]
async fn operator_metrics_capture_batches_and_submissions() {
    let rollup = Arc::new(
        Rollup::new(zk_rollup_config("operator-metrics"))
            .await
            .expect("rollup init"),
    );
    let mut operator = RollupOperator::new(
        OperatorConfig {
            batch_interval: 1,
            submission_interval: 1,
            auto_submit: true,
            ..OperatorConfig::default()
        },
        rollup,
    );

    operator.start().await.expect("start operator");
    sleep(Duration::from_millis(2_200)).await;

    let metrics = operator.get_metrics().await;
    assert!(metrics.batches_created >= 1);
    assert!(metrics.batches_submitted >= 1);
    assert!(metrics.last_batch_timestamp.is_some());
    assert!(metrics.last_submission_timestamp.is_some());
}

#[tokio::test]
async fn operator_submit_transaction_surfaces_rollup_validation_errors() {
    let rollup = Arc::new(
        Rollup::new(zk_rollup_config("operator-validation"))
            .await
            .expect("rollup init"),
    );
    let mut operator = RollupOperator::new(
        OperatorConfig {
            batch_interval: 1,
            submission_interval: 30,
            auto_submit: false,
            ..OperatorConfig::default()
        },
        rollup,
    );

    operator.start().await.expect("start operator");
    sleep(Duration::from_millis(1_100)).await;

    let tx = self_transfer_tx(addr(32), 10, 1, 0, 12);
    let err = operator
        .submit_transaction(tx)
        .await
        .expect_err("self transfer must fail rollup validation");
    assert!(
        err.to_string()
            .contains("Sender and recipient must be different")
    );
}

#[tokio::test]
async fn operator_handles_concurrent_invalid_submissions_without_leaving_active_state() {
    let rollup = Arc::new(
        Rollup::new(zk_rollup_config("operator-concurrency"))
            .await
            .expect("rollup init"),
    );
    let mut operator = RollupOperator::new(
        OperatorConfig {
            batch_interval: 1,
            auto_submit: false,
            ..OperatorConfig::default()
        },
        rollup,
    );

    operator.start().await.expect("start operator");
    sleep(Duration::from_millis(1_100)).await;

    let operator = Arc::new(operator);
    let mut tasks = JoinSet::new();
    for nonce in 0..10u64 {
        let operator = Arc::clone(&operator);
        tasks.spawn(async move {
            operator
                .submit_transaction(self_transfer_tx(addr(40), 1, 0, nonce, 20 + nonce as i64))
                .await
        });
    }

    let mut failures = 0usize;
    while let Some(result) = tasks.join_next().await {
        let submit_result = result.expect("task join");
        if submit_result.is_err() {
            failures += 1;
        }
    }

    assert_eq!(failures, 10);
    assert_eq!(operator.get_status().await, OperatorStatus::Active);
}

// Channel tests

#[tokio::test]
async fn channel_open_update_sign_close_settle_happy_path() {
    let alice = addr(0xa1);
    let bob = addr(0xb1);
    let channel = StateChannel::new(
        "channel-happy".to_string(),
        vec![
            (alice, vk_from_seed(0xa1), 600),
            (bob, vk_from_seed(0xb1), 400),
        ],
        ChannelType::StateChannel,
        12,
    )
    .await
    .expect("create channel");

    channel.open().await.expect("open channel");
    let mut balances = HashMap::new();
    balances.insert(alice, 550);
    balances.insert(bob, 450);
    let sequence = channel
        .propose_update(&alice, balances.clone(), Some(vec![1, 2, 3]), vec![0xaa])
        .await
        .expect("propose update");
    channel
        .sign_update(&bob, sequence, vec![0xbb])
        .await
        .expect("second signer");
    channel
        .initiate_closure(&alice)
        .await
        .expect("close channel");
    let settled = channel.settle().await.expect("settle channel");

    assert_eq!(settled, balances);
    assert_eq!(
        channel.get_state().await.expect("channel state").status,
        ChannelStatus::Settled
    );
}

#[tokio::test]
async fn channel_propose_update_rejects_balance_sum_mismatch() {
    let alice = addr(0xa2);
    let bob = addr(0xb2);
    let channel = StateChannel::new(
        "channel-balance".to_string(),
        vec![
            (alice, vk_from_seed(0xa2), 500),
            (bob, vk_from_seed(0xb2), 500),
        ],
        ChannelType::PaymentChannel,
        8,
    )
    .await
    .expect("create channel");
    channel.open().await.expect("open channel");

    let mut balances = HashMap::new();
    balances.insert(alice, 800);
    balances.insert(bob, 300);
    let err = channel
        .propose_update(&alice, balances, None, vec![1])
        .await
        .expect_err("must reject mismatch");
    assert!(err.to_string().contains("Balance sum mismatch"));
}

#[tokio::test]
async fn channel_propose_update_requires_all_participant_balances() {
    let alice = addr(0xa3);
    let bob = addr(0xb3);
    let channel = StateChannel::new(
        "channel-participants".to_string(),
        vec![
            (alice, vk_from_seed(0xa3), 700),
            (bob, vk_from_seed(0xb3), 300),
        ],
        ChannelType::StateChannel,
        9,
    )
    .await
    .expect("create channel");
    channel.open().await.expect("open channel");

    let mut balances = HashMap::new();
    balances.insert(alice, 1000);
    let err = channel
        .propose_update(&alice, balances, None, vec![2])
        .await
        .expect_err("missing participant balance must reject");
    assert!(err.to_string().contains("Missing balance for participant"));
}

#[tokio::test]
async fn channel_sign_update_rejects_unknown_participant() {
    let alice = addr(0xa4);
    let bob = addr(0xb4);
    let outsider = addr(0xc4);
    let channel = StateChannel::new(
        "channel-unknown-signer".to_string(),
        vec![
            (alice, vk_from_seed(0xa4), 500),
            (bob, vk_from_seed(0xb4), 500),
        ],
        ChannelType::StateChannel,
        6,
    )
    .await
    .expect("create channel");
    channel.open().await.expect("open channel");

    let mut balances = HashMap::new();
    balances.insert(alice, 400);
    balances.insert(bob, 600);
    let sequence = channel
        .propose_update(&alice, balances, None, vec![3])
        .await
        .expect("propose update");

    let err = channel
        .sign_update(&outsider, sequence, vec![4])
        .await
        .expect_err("outsider must reject");
    assert!(
        err.to_string()
            .contains("Signer is not a channel participant")
    );
}

#[tokio::test]
async fn channel_sign_update_rejects_missing_sequence() {
    let alice = addr(0xa5);
    let bob = addr(0xb5);
    let channel = StateChannel::new(
        "channel-missing-sequence".to_string(),
        vec![
            (alice, vk_from_seed(0xa5), 500),
            (bob, vk_from_seed(0xb5), 500),
        ],
        ChannelType::StateChannel,
        6,
    )
    .await
    .expect("create channel");
    channel.open().await.expect("open channel");

    let err = channel
        .sign_update(&bob, 99, vec![5])
        .await
        .expect_err("missing sequence must reject");
    assert!(err.to_string().contains("State update not found"));
}

#[tokio::test]
async fn channel_balance_invariant_holds_after_updates_and_settlement() {
    let alice = addr(0xa6);
    let bob = addr(0xb6);
    let channel = StateChannel::new(
        "channel-invariant".to_string(),
        vec![
            (alice, vk_from_seed(0xa6), 900),
            (bob, vk_from_seed(0xb6), 100),
        ],
        ChannelType::PaymentChannel,
        10,
    )
    .await
    .expect("create channel");
    channel.open().await.expect("open channel");

    let mut balances = HashMap::new();
    balances.insert(alice, 500);
    balances.insert(bob, 500);
    channel
        .propose_update(&alice, balances.clone(), None, vec![6])
        .await
        .expect("propose update");
    channel.initiate_closure(&bob).await.expect("close channel");
    let settled = channel.settle().await.expect("settle");

    let sum: u64 = settled.values().sum();
    assert_eq!(sum, 1_000);
}

#[tokio::test]
async fn channel_generate_state_hash_is_stable_for_same_sequence() {
    let alice = addr(0xa7);
    let bob = addr(0xb7);
    let channel = StateChannel::new(
        "channel-hash-repeat".to_string(),
        vec![
            (alice, vk_from_seed(0xa7), 750),
            (bob, vk_from_seed(0xb7), 250),
        ],
        ChannelType::StateChannel,
        7,
    )
    .await
    .expect("create channel");
    channel.open().await.expect("open channel");

    let mut balances = HashMap::new();
    balances.insert(alice, 700);
    balances.insert(bob, 300);
    let sequence = channel
        .propose_update(&alice, balances, Some(vec![7, 7]), vec![7])
        .await
        .expect("propose update");

    let first = channel
        .generate_state_hash(sequence)
        .await
        .expect("hash one");
    let second = channel
        .generate_state_hash(sequence)
        .await
        .expect("hash two");
    assert_eq!(first, second);
}

#[tokio::test]
async fn channel_disputed_settlement_uses_latest_counterevidence_update() {
    let alice = addr(0xa8);
    let bob = addr(0xb8);
    let channel = StateChannel::new(
        "channel-dispute".to_string(),
        vec![
            (alice, vk_from_seed(0xa8), 500),
            (bob, vk_from_seed(0xb8), 500),
        ],
        ChannelType::StateChannel,
        11,
    )
    .await
    .expect("create channel");
    channel.open().await.expect("open channel");

    {
        let mut updates = channel.state_updates.write().await;
        updates.push(ChannelStateUpdate {
            sequence: 1,
            balances: HashMap::from([(alice, 300), (bob, 700)]),
            timestamp: 1_710_000_001,
            state_data: Some(vec![1]),
            signatures: HashMap::from([(alice, vec![1]), (bob, vec![2])]),
        });
        updates.push(ChannelStateUpdate {
            sequence: 2,
            balances: HashMap::from([(alice, 650), (bob, 350)]),
            timestamp: 1_710_000_002,
            state_data: Some(vec![2]),
            signatures: HashMap::from([(alice, vec![3]), (bob, vec![4])]),
        });
    }
    *channel.current_sequence.write().await = 2;
    *channel.status.write().await = ChannelStatus::Disputed;

    let settled = channel.settle().await.expect("settle disputed channel");
    assert_eq!(settled.get(&alice).copied(), Some(650));
    assert_eq!(settled.get(&bob).copied(), Some(350));
}

#[tokio::test]
async fn channel_records_signature_bytes_without_validation_today() {
    let alice = addr(0xa9);
    let bob = addr(0xb9);
    let channel = StateChannel::new(
        "channel-signature-gap".to_string(),
        vec![
            (alice, vk_from_seed(0xa9), 600),
            (bob, vk_from_seed(0xb9), 400),
        ],
        ChannelType::StateChannel,
        5,
    )
    .await
    .expect("create channel");
    channel.open().await.expect("open channel");

    let mut balances = HashMap::new();
    balances.insert(alice, 580);
    balances.insert(bob, 420);
    let sequence = channel
        .propose_update(&alice, balances, None, Vec::new())
        .await
        .expect("current implementation accepts empty signature bytes");
    channel
        .sign_update(&bob, sequence, Vec::new())
        .await
        .expect("current implementation accepts empty signer bytes");

    let state = channel.get_state().await.expect("channel state");
    assert_eq!(state.current_sequence, 1);
}

// State consistency

#[tokio::test]
async fn state_apply_same_sequence_produces_same_root() {
    let alice = addr(0xc1);
    let bob = addr(0xd1);
    let txs = vec![
        transfer_tx(alice, bob, 10, 1, 0, 30),
        transfer_tx(alice, bob, 5, 1, 1, 31),
    ];

    let mut first = funded_state(&[(alice, 100), (bob, 0)]);
    let mut second = funded_state(&[(alice, 100), (bob, 0)]);
    for tx in &txs {
        first
            .apply_transaction(tx)
            .await
            .expect("apply tx to first");
        second
            .apply_transaction(tx)
            .await
            .expect("apply tx to second");
    }

    assert_eq!(first.get_state_root(), second.get_state_root());
}

#[test]
fn account_hash_sorts_storage_keys_for_determinism() {
    let account = Layer2Account {
        address: addr(0xc2),
        balance: 50,
        nonce: 7,
        storage: HashMap::from([
            ("z".to_string(), vec![3]),
            ("a".to_string(), vec![1]),
            ("m".to_string(), vec![2]),
        ]),
        code: Some(vec![9, 9]),
        last_update: 123,
    };
    let account_reordered = Layer2Account {
        storage: HashMap::from([
            ("m".to_string(), vec![2]),
            ("z".to_string(), vec![3]),
            ("a".to_string(), vec![1]),
        ]),
        ..account.clone()
    };

    assert_eq!(account.hash(), account_reordered.hash());
}

#[tokio::test]
async fn state_apply_transaction_rolls_back_on_error() {
    let alice = addr(0xc3);
    let bob = addr(0xd3);
    let mut state = funded_state(&[(alice, 15), (bob, 20)]);
    let before = state.clone();
    let tx = transfer_tx(alice, bob, 20, 1, 0, 32);

    let err = state
        .apply_transaction(&tx)
        .await
        .expect_err("insufficient balance must reject");
    assert!(err.to_string().contains("Insufficient balance"));
    assert_eq!(
        state.accounts.get(&alice).unwrap().balance,
        before.accounts.get(&alice).unwrap().balance
    );
    assert_eq!(
        state.accounts.get(&alice).unwrap().nonce,
        before.accounts.get(&alice).unwrap().nonce
    );
    assert_eq!(
        state.accounts.get(&bob).unwrap().balance,
        before.accounts.get(&bob).unwrap().balance
    );
}

#[test]
fn state_generate_account_proof_returns_none_for_unknown_account() {
    let state = Layer2State::new();
    assert!(state.generate_account_proof(&addr(0xc4)).is_none());
}

#[tokio::test]
async fn state_generate_account_proof_roundtrips_via_bincode() {
    let alice = addr(0xc5);
    let mut state = funded_state(&[(alice, 100)]);
    let tx = self_transfer_tx(alice, 5, 0, 0, 33);
    state
        .apply_transaction(&tx)
        .await
        .expect("apply self transfer for root update");

    let proof = state
        .generate_account_proof(&alice)
        .expect("proof for existing account");
    let bytes = bincode::serialize(&proof).expect("serialize proof");
    let decoded: StateProof = bincode::deserialize(&bytes).expect("deserialize proof");
    assert_eq!(decoded.proof_type, StateProofType::MerkleProof);
    assert_eq!(decoded.proof_data, proof.proof_data);
}

#[tokio::test]
async fn state_serialization_roundtrip_preserves_full_state() {
    let alice = addr(0xc6);
    let bob = addr(0xd6);
    let mut state = funded_state(&[(alice, 100), (bob, 20)]);
    let tx = transfer_tx(alice, bob, 10, 1, 0, 34);
    state
        .apply_transaction(&tx)
        .await
        .expect("apply transaction");

    let bytes = bincode::serialize(&state).expect("serialize state");
    let decoded: Layer2State = bincode::deserialize(&bytes).expect("deserialize state");
    assert_eq!(decoded.get_state_root(), state.get_state_root());
    assert_eq!(decoded.accounts.len(), state.accounts.len());
}

#[tokio::test]
async fn verify_state_transition_accepts_current_root_and_rejects_stale_root() {
    let alice = addr(0xc7);
    let bob = addr(0xd7);
    let mut state = funded_state(&[(alice, 100), (bob, 0)]);
    let tx = transfer_tx(alice, bob, 10, 1, 0, 35);
    let old_root = state.get_state_root();
    state
        .apply_transaction(&tx)
        .await
        .expect("apply transaction");
    let new_root = state.get_state_root();

    let good = StateTransition {
        batch_id: 1,
        from_state_root: new_root,
        to_state_root: new_root,
        transaction_count: 1,
        timestamp: 1_710_000_010,
        proof: StateProof::default(),
    };
    let stale = StateTransition {
        batch_id: 2,
        from_state_root: old_root,
        to_state_root: new_root,
        transaction_count: 1,
        timestamp: 1_710_000_011,
        proof: StateProof::default(),
    };

    assert!(
        state
            .verify_state_transition(&good)
            .expect("good transition")
    );
    assert!(
        !state
            .verify_state_transition(&stale)
            .expect("stale transition")
    );
}

#[tokio::test]
async fn state_self_transfer_preserves_balance_minus_fee_and_increments_nonce() {
    let alice = addr(0xc8);
    let mut state = funded_state(&[(alice, 50)]);
    let tx = self_transfer_tx(alice, 5, 2, 0, 36);
    state
        .apply_transaction(&tx)
        .await
        .expect("self transfer is allowed at state layer");

    let account = state.get_account(&alice).expect("account exists");
    assert_eq!(account.balance, 48);
    assert_eq!(account.nonce, 1);
}

// Cross-module determinism

#[tokio::test]
async fn state_root_is_independent_of_account_insertion_order() {
    let alice = addr(0xe1);
    let bob = addr(0xf1);
    let tx = transfer_tx(alice, bob, 10, 0, 0, 40);

    let mut first = Layer2State::new();
    first.accounts.insert(alice, Layer2Account::new(alice));
    first.accounts.insert(bob, Layer2Account::new(bob));
    first.accounts.get_mut(&alice).unwrap().balance = 100;

    let mut second = Layer2State::new();
    second.accounts.insert(bob, Layer2Account::new(bob));
    second.accounts.insert(alice, Layer2Account::new(alice));
    second.accounts.get_mut(&alice).unwrap().balance = 100;

    first.apply_transaction(&tx).await.expect("apply first");
    second.apply_transaction(&tx).await.expect("apply second");
    assert_eq!(first.get_state_root(), second.get_state_root());
}

#[tokio::test]
async fn channel_state_hash_is_independent_of_balance_map_insertion_order() {
    let alice = addr(0xe2);
    let bob = addr(0xf2);
    let first = StateChannel::new(
        "channel-order".to_string(),
        vec![
            (alice, vk_from_seed(0xe2), 600),
            (bob, vk_from_seed(0xf2), 400),
        ],
        ChannelType::StateChannel,
        5,
    )
    .await
    .expect("create first");
    let second = StateChannel::new(
        "channel-order".to_string(),
        vec![
            (bob, vk_from_seed(0xf2), 400),
            (alice, vk_from_seed(0xe2), 600),
        ],
        ChannelType::StateChannel,
        5,
    )
    .await
    .expect("create second");
    first.open().await.expect("open first");
    second.open().await.expect("open second");

    {
        let mut first_updates = first.state_updates.write().await;
        first_updates.push(ChannelStateUpdate {
            sequence: 1,
            balances: HashMap::from([(alice, 450), (bob, 550)]),
            timestamp: 1_710_000_100,
            state_data: Some(vec![9]),
            signatures: HashMap::new(),
        });
    }
    {
        let mut second_updates = second.state_updates.write().await;
        second_updates.push(ChannelStateUpdate {
            sequence: 1,
            balances: HashMap::from([(bob, 550), (alice, 450)]),
            timestamp: 1_710_000_100,
            state_data: Some(vec![9]),
            signatures: HashMap::new(),
        });
    }
    *first.current_sequence.write().await = 1;
    *second.current_sequence.write().await = 1;

    let first_hash = first.generate_state_hash(1).await.expect("hash first");
    let second_hash = second.generate_state_hash(1).await.expect("hash second");
    assert_eq!(first_hash, second_hash);
}

#[tokio::test]
async fn verifier_manual_results_are_cached_after_first_verification() {
    let (rollup, batch_id) = ready_empty_batch_rollup("verifier-cache").await;
    let batch = rollup.get_batch(batch_id).await.expect("batch exists");
    let verifier = active_verifier(VerificationMode::ZKProof).await;
    verifier
        .add_batch_for_verification(batch)
        .await
        .expect("add batch");

    let first = verifier
        .verify_batch_manually(batch_id)
        .await
        .expect("first verification");
    let second = verifier
        .verify_batch_manually(batch_id)
        .await
        .expect("cached verification");

    assert!(first.success);
    assert_eq!(first.batch_id, second.batch_id);
    assert_eq!(
        verifier
            .get_verification_result(batch_id)
            .await
            .expect("cached result")
            .batch_id,
        batch_id
    );
}

#[tokio::test]
async fn verifier_manual_verification_rejects_tampered_rollup_proof() {
    let (rollup, batch_id) = ready_empty_batch_rollup("verifier-tamper").await;
    let mut batch = rollup.get_batch(batch_id).await.expect("batch exists");
    batch.zkproof.as_mut().expect("zk proof").proof_data[0] ^= 0xff;

    let verifier = active_verifier(VerificationMode::ZKProof).await;
    verifier
        .add_batch_for_verification(batch)
        .await
        .expect("add batch");

    let result = verifier
        .verify_batch_manually(batch_id)
        .await
        .expect("verification result");
    assert!(!result.success);
}

#[tokio::test]
async fn verifier_rejects_duplicate_pending_or_verified_batches() {
    let (rollup, batch_id) = ready_empty_batch_rollup("verifier-duplicates").await;
    let batch = rollup.get_batch(batch_id).await.expect("batch exists");
    let verifier = active_verifier(VerificationMode::ZKProof).await;

    verifier
        .add_batch_for_verification(batch.clone())
        .await
        .expect("first add");
    let pending_err = verifier
        .add_batch_for_verification(batch.clone())
        .await
        .expect_err("duplicate pending must reject");
    assert!(pending_err.to_string().contains("already pending"));

    verifier
        .verify_batch_manually(batch_id)
        .await
        .expect("manual verify");
    let verified_err = verifier
        .add_batch_for_verification(batch)
        .await
        .expect_err("duplicate verified must reject");
    assert!(verified_err.to_string().contains("already verified"));
}
