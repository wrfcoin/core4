//! The producer's re-prune liveness guard: when a selection step removes a
//! transaction after the first trial prune, anything that leaned on it (here,
//! a same-block reveal whose commit was removed) is dropped and evicted by a
//! second trial prune, instead of making `create_next_block` refuse its own
//! block. Uses the dev-only `consensus-test-hooks` stand-in for a selection
//! step, so it owns its process.
//!
//! Run from core4:
//!   cargo test -p wrfcoin-shared-protocol --test producer_reprune_guard

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::RwLock;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, ForecastArtifactManifest, RevealPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    ProofOfPublishing, hold_after_first_prune_for_tests,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}

/// A commit + reveal pair for `station_id` (`kind` is "forecast" or
/// "contribution"), signed by a throwaway key, as the earn-loop tests build
/// them.
fn commit_and_reveal(station_id: &str, kind: &str, tag: &str) -> (Transaction, Transaction) {
    let data = serde_json::json!({
        "quality": 1.0, "volume": 1.0, "uniqueness": 1.0,
        "temperature_c": 21.5, "humidity_percent": 60.0, "pressure_hpa": 1014.0,
    });
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(&data, &nonce, station_id, timestamp, kind);
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: kind.to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(&key(0x77));
    let reveal = RevealPayload {
        commit_id,
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: kind.to_string(),
        data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        // A forecast reveal must carry an artifact manifest.
        manifest: (kind == "forecast").then(|| ForecastArtifactManifest {
            schema_version: 1,
            model_id: "wrf-gfs-v3".to_string(),
            input_hash: "a".repeat(64),
            output_hash: "b".repeat(64),
            produced_at: timestamp,
            region: "us-east".to_string(),
            horizon_hours: 48,
            input_variables: vec!["pressure".to_string(), "temperature".to_string()],
            output_variables: vec!["temperature".to_string()],
        }),
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };
    let (commit_type, reveal_type) = if kind == "forecast" {
        (
            TransactionType::ForecastCommit,
            TransactionType::ForecastReveal,
        )
    } else {
        (
            TransactionType::ContributionCommit,
            TransactionType::ContributionReveal,
        )
    };
    let envelope = |tx_type: TransactionType, data: String, tag: String| {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&tag),
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
        tx.data = Some(data);
        tx.fee = 1;
        tx.set_id();
        tx
    };
    (
        envelope(
            commit_type,
            serde_json::to_string(&commit).unwrap(),
            format!("commit-{tag}"),
        ),
        envelope(
            reveal_type,
            serde_json::to_string(&reveal).unwrap(),
            format!("reveal-{tag}"),
        ),
    )
}

#[tokio::test]
async fn a_reveal_stranded_by_a_removal_after_the_first_prune_is_evicted_not_fatal() {
    let mut rng = StdRng::seed_from_u64(21);
    let miner = PrivateKey::generate(&mut rng);
    let miner_hex = format!(
        "0x{}",
        hex::encode(
            wrfcoin_shared_protocol::common::types::Address::from_public_key(
                &PublicKey::from_private_key(&miner)
            )
            .0
        )
    );
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
        .await
        .unwrap();

    let (held_commit, stranded_reveal) = commit_and_reveal("ws-guard-held", "contribution", "held");
    let (other_commit, other_reveal) = commit_and_reveal("ws-guard-other", "contribution", "other");
    for tx in [&held_commit, &stranded_reveal, &other_commit, &other_reveal] {
        let keys = chain.read().await.state.signing_keys.clone();
        mempool
            .write()
            .await
            .add_transaction_with_signing_keys(tx.clone(), Some(&keys))
            .expect("admitted to the mempool");
    }
    let id = |tx: &Transaction| tx.id.unwrap();

    // A selection step removes the commit after the first prune; its reveal
    // stays in the candidate.
    hold_after_first_prune_for_tests(vec![id(&held_commit)]);
    pop.create_next_block(chain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("a stranded reveal never makes the producer refuse its block");
    hold_after_first_prune_for_tests(Vec::new());

    let c = chain.read().await;
    assert_eq!(c.height, 2);
    let mined: Vec<_> = c
        .blocks()
        .values()
        .last()
        .unwrap()
        .transactions
        .iter()
        .filter_map(|t| t.id)
        .collect();
    let pool = mempool.read().await;
    // The stranded reveal: not mined, and evicted.
    assert!(!mined.contains(&id(&stranded_reveal)));
    assert!(
        !pool.contains(&id(&stranded_reveal)),
        "the stranded reveal is evicted"
    );
    // The held commit: not mined, still pending (a removal is not an eviction).
    assert!(!mined.contains(&id(&held_commit)));
    assert!(
        pool.contains(&id(&held_commit)),
        "the held commit stays in the mempool"
    );
    // Nothing else is touched: the unrelated pair is mined.
    assert!(
        mined.contains(&id(&other_commit)),
        "the unrelated commit is mined"
    );
    assert!(
        mined.contains(&id(&other_reveal)),
        "the unrelated reveal is mined"
    );
    assert_eq!(pool.size(), 1, "only the held commit is left");
}
