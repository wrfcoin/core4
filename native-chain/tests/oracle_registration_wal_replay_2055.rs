//! #2055 Option A — a fork-safe on-chain oracle registration survives WAL
//! replay identically.
//!
//! A producer that applies the registration and a follower that reconstructs it
//! from the WAL alone on restart (`Storage::load_state`, no checkpoint) MUST
//! converge on the same `state_root`, with the operator present in the oracle
//! set on both. This is the WAL-replay leg of the producer == applier == replay
//! determinism the feature depends on (companion to the P2P-apply parity test
//! `producer_applier_block_apply_parity_oracle_registration`).

use rand::SeedableRng;
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;

fn tmp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("wrfcoin-2055-{}.redb", fastrand::u64(..)))
}

/// Oracle ids must be valid ed25519 public keys (the config apply derives a
/// legacy key from each), so use a real key's public half.
fn oracle_identity() -> String {
    let sk = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(2_055_999));
    PublicKey::from_private_key(&sk).to_hex()
}

fn registration_transition(identity: &str) -> StateTransition {
    StateTransition::ApplyOracleRegistration {
        oracle_set: vec![identity.to_string()],
        oracle_quorum: 1,
        identity: identity.to_string(),
        public_key: identity.to_string(),
        epoch: 2,
        activated_at: 1_700_000_000,
    }
}

#[tokio::test]
async fn oracle_registration_survives_wal_replay_2055() {
    // Astronomical interval → no checkpoint is written, so recovery is PURE WAL
    // replay from genesis.
    const INTERVAL: u64 = u64::MAX / 2;
    let identity = oracle_identity();

    // --- Producer: apply the registration transition and finalize. ---
    let mut producer = Blockchain::default();
    producer
        .apply_state_transition(registration_transition(&identity))
        .expect("producer applies oracle registration");
    producer.state.apply_block_finalization(1);
    assert!(
        producer.state.oracle_set.contains(&identity),
        "producer must have the oracle registered"
    );
    assert_eq!(producer.state.oracle_quorum, 1);
    let producer_root = producer.state.state_root.0;

    // --- Persist the block + its transition to the WAL, no checkpoint. ---
    let path = tmp_db();
    {
        let mut storage = Storage::open(&path, INTERVAL).expect("open storage");
        let mut bc = Blockchain::default();
        let transition = registration_transition(&identity);
        bc.apply_state_transition(transition.clone())
            .expect("persist-side transition applies");
        bc.state.apply_block_finalization(1);

        let mut block = Block::new(1, bc.tip, vec![], 1, 1);
        let hash = block.calculate_hash();
        block.set_hash(hash);
        bc.add_block(block.clone()).expect("add_block");

        let checkpoint = storage
            .checkpoint_state_bytes(1, &bc.state)
            .expect("checkpoint_state_bytes");
        assert!(checkpoint.is_none(), "interval must never checkpoint");
        storage
            .persist_block(block, vec![transition], checkpoint)
            .await
            .expect("persist_block");
    }

    // --- Follower restart: recover via WAL replay only. ---
    let reloaded = Storage::open(&path, INTERVAL)
        .expect("re-open storage")
        .load_state()
        .expect("load_state must not error")
        .expect("load_state returns Some after writing a block");
    let _ = std::fs::remove_file(&path);

    assert!(
        reloaded.state.oracle_set.contains(&identity),
        "WAL replay must reproduce the oracle registration"
    );
    assert_eq!(
        reloaded.state.oracle_quorum, 1,
        "WAL replay must reproduce the oracle quorum"
    );
    assert_eq!(
        reloaded.state.state_root.0, producer_root,
        "producer and WAL-replay state_root must match — fork-safety (#2055)"
    );
}
