//! #2464 — a late-quorum contribution reward survives a checkpoint-less restart.
//!
//! The producer-path timing is exercised by
//! `earn_loop_oracle_gate_e2e::late_quorum_finalizes_the_durable_reward_decision`.
//! This integration test carries that same deferred record through the separate
//! storage boundary: pending at h1, quorum plus settlement at h2, then pure WAL
//! replay. It must not pass merely because a checkpoint serializes live state.

use rand::{SeedableRng, rngs::StdRng};
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::state::{
    ContributionRewardRecord, StateTransition,
};
use wrfcoin_shared_protocol::blockchain::transaction::OracleAttestationPayload;
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

const NEVER_CHECKPOINT: u64 = u64::MAX / 2;
const COMMIT_ID: &str = "2464-late-quorum-wal-replay";
const RECIPIENT: Address = Address([0x46; 20]);

fn tmp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("wrfcoin-2464-{}.redb", fastrand::u64(..)))
}

fn oracle(seed: u64) -> (PrivateKey, String) {
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    (private_key.clone(), PublicKey::from_private_key(&private_key).to_string())
}

fn attestation(
    private_key: &PrivateKey,
    oracle_id: &str,
    timestamp: i64,
) -> OracleAttestationPayload {
    let mut payload = OracleAttestationPayload {
        commit_id: COMMIT_ID.to_string(),
        oracle_id: oracle_id.to_string(),
        approved: true,
        signature: None,
        confidence: Some(1.0),
        timestamp,
        nonce: Some(format!("{oracle_id}:{COMMIT_ID}:{timestamp}")),
    };
    payload.signature = Some(private_key.sign(payload.signing_message().as_bytes()).to_string());
    payload
}

fn deferred_record() -> ContributionRewardRecord {
    ContributionRewardRecord {
        address: RECIPIENT,
        amount: Balance::new(25),
        reason: format!("contribution:{COMMIT_ID}"),
        station_id: "station-2464-wal".to_string(),
        provenance_class: ContributionProvenanceClass::Real,
    }
}

async fn persist_block(
    storage: &mut Storage,
    blockchain: &mut Blockchain,
    height: u64,
    transitions: Vec<StateTransition>,
) {
    for transition in &transitions {
        blockchain
            .apply_state_transition(transition.clone())
            .expect("producer-side transition applies");
    }
    blockchain.state.apply_block_finalization(height);
    let mut block = Block::new(1, blockchain.tip, vec![], 1, height);
    block.set_hash(block.calculate_hash());
    blockchain.add_block(block.clone()).expect("add block");
    let checkpoint = storage
        .checkpoint_state_bytes(height, &blockchain.state)
        .expect("checkpoint bytes");
    assert!(checkpoint.is_none(), "control: recovery must use WAL, not a checkpoint");
    storage
        .persist_block(block, transitions, checkpoint)
        .await
        .expect("persist block and transitions");
}

#[tokio::test]
async fn late_quorum_reward_survives_pure_wal_replay_2464() {
    let (oracle_a_key, oracle_a) = oracle(2_464_001);
    let (oracle_b_key, oracle_b) = oracle(2_464_002);
    let path = tmp_db();

    let producer_root;
    {
        let mut storage = Storage::open(&path, NEVER_CHECKPOINT).expect("open storage");
        let mut producer = Blockchain::default();

        // h1: the real contribution decision is durable but cannot pay before
        // quorum. This is the deferred state #2464 must preserve.
        persist_block(
            &mut storage,
            &mut producer,
            1,
            vec![
                StateTransition::SetOracleConfig {
                    oracle_set: vec![oracle_a.clone(), oracle_b.clone()],
                    oracle_quorum: 2,
                },
                StateTransition::ApplyContributionRewardDecision {
                    record: deferred_record(),
                    current_block: 1,
                },
            ],
        )
        .await;
        assert!(
            !producer.state.finalized_contribution_rewards.contains(COMMIT_ID),
            "control: pre-quorum reward must remain unpaid at h1"
        );

        // h2: independent signed attestations reach quorum, then the producer
        // emits the same typed reward decision to settle the previously pending
        // record. No direct balance mutation is used.
        persist_block(
            &mut storage,
            &mut producer,
            2,
            vec![
                StateTransition::RecordOracleAttestation {
                    payload: attestation(&oracle_a_key, &oracle_a, 2),
                    current_block: 2,
                },
                StateTransition::RecordOracleAttestation {
                    payload: attestation(&oracle_b_key, &oracle_b, 3),
                    current_block: 2,
                },
                StateTransition::ApplyContributionRewardDecision {
                    record: deferred_record(),
                    current_block: 2,
                },
            ],
        )
        .await;
        assert!(producer.state.finalized_contribution_rewards.contains(COMMIT_ID));
        assert_eq!(
            producer.state.get_account(&RECIPIENT).unwrap().earned_balance_total,
            Balance::new(25),
            "producer must credit exactly one late-quorum reward"
        );
        producer_root = producer.state.state_root.0;
    }

    let recovered = Storage::open(&path, NEVER_CHECKPOINT)
        .expect("reopen storage")
        .load_state()
        .expect("WAL replay must not error")
        .expect("WAL replay must recover the two blocks");
    let _ = std::fs::remove_file(&path);

    assert_eq!(recovered.height, 2, "replay must recover the settlement block");
    assert!(
        recovered.state.finalized_contribution_rewards.contains(COMMIT_ID),
        "WAL replay must retain the exactly-once finalization marker"
    );
    assert_eq!(
        recovered.state.get_account(&RECIPIENT).unwrap().earned_balance_total,
        Balance::new(25),
        "WAL replay must retain the contributor credit"
    );
    assert_eq!(
        recovered.state.state_root.0, producer_root,
        "producer and WAL-replay state roots must converge for late quorum settlement"
    );
}
