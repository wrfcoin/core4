//! #3290: protocol-level forecaster key binding (`forecaster_key_binding`).
//!
//! The probe from the issue: a forecast commit for a victim forecaster, whose
//! authorization is signed by a freshly generated, unrelated key. Before the
//! binding activates (`H − 1`) that commit is admitted, included and
//! registered exactly as on main. From `H` it is refused at mempool admission
//! (route and gossip share `add_transaction_with_signing_keys_at_height`), a
//! block carrying it is refused by `verify_block`, and a block the real
//! producer assembles drops it. A commit whose sender IS the address of its
//! key is accepted on both sides of `H` (the control).
//!
//! Parity, with the feature active from `H`:
//! - **producer**: the real `ProofOfPublishing::create_next_block`;
//! - **applier**: `extract_transitions` → `apply_transitions_batch_at_height`;
//! - **WAL**: every block persisted through `Storage::persist_block`, then the
//!   stored transitions replayed from genesis.
//!
//! The feature is scheduled through the process-wide extension overlay (the
//! way the node installs an applied extension), so the scenario runs alone in
//! a child process and the parent never touches the overlay. The gate-off
//! test runs in the parent, where nothing is scheduled.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;

mod helpers {
    pub(crate) mod test_env {
        // Mount the canonical test-only lock and fixture pins (#3521).
        include!("../src/bin/launch_api/test_env.rs");
    }
}

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use rustc_hash::FxHashSet;
use tokio::sync::RwLock;
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::consensus::proof_of_publishing::{
    ProofOfPublishing, adjusted_block_reward,
};
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{PopLivenessPolicy, State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, feature_active, refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_bytes};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const H: u64 = 3;
const FEATURE: &str = "forecaster_key_binding";
const CHILD_ENV: &str = "WRFCOIN_FORECASTER_KEY_BINDING_CHILD_3290";
const TEST_NAME: &str = "forecaster_key_binding_refuses_unbound_commits_from_activation";
const STATION: &str = "fcst-cell-0001";

fn key(seed: u64) -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(seed))
}

fn address_of(key: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(key))
}

/// A forecast commit shaped exactly like `enqueue_protocol_transaction`
/// builds it: `forecaster` is the input label, the output address and (when
/// it parses) `sender_address`. The authorization is signed by `signer`.
fn forecast_commit(forecaster: &str, signer: &PrivateKey, tag: &str) -> Transaction {
    let mut payload = CommitPayload {
        commit_id: hash_bytes(tag.as_bytes()).to_string(),
        station_id: STATION.to_string(),
        timestamp: chrono::Utc::now().timestamp(),
        contribution_type: "forecast".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    payload.sign_authorization(signer);
    let inputs = vec![TransactionInput {
        txid: hash_bytes(format!("input:{tag}").as_bytes()),
        vout: 0,
        signature: None,
        public_key: Some(forecaster.to_string()),
        coinbase_data: None,
    }];
    let outputs = vec![TransactionOutput {
        address: forecaster.to_string(),
        amount: 0,
        script: "OP_TRUE".to_string(),
        nft_data: None,
    }];
    let mut tx = Transaction::new(inputs, outputs);
    tx.tx_type = TransactionType::ForecastCommit;
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    if let Ok(address) = forecaster.parse::<Address>() {
        tx.sender_address = address;
    }
    tx.set_id();
    assert!(tx.verify_commit_authorization(), "the probe's signature itself is valid");
    tx
}

fn commit_id(tx: &Transaction) -> String {
    tx.parse_commit_payload().unwrap().commit_id
}

/// The #1762 recipe: a hand-built block carrying `tx`, accepted by every gate
/// except the one under test.
async fn verify_block_with(tx: Transaction, height: u64) -> bool {
    let txs = vec![
        Transaction::new_coinbase("fkb-3290-miner", adjusted_block_reward(height), height),
        tx,
    ];
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    pop.set_pop_liveness_policy(PopLivenessPolicy {
        useful_work_activation_height: 0,
    })
    .expect("liveness policy");
    let score = pop.calculate_block_publishing_score(&txs, height);
    let mut block = Block::new(3, Hash::default(), txs, 1, height);
    block.metadata.insert("publishing_score".to_string(), format!("{score}"));
    pop.verify_block(&block, &FxHashSet::default(), 2, &[])
        .await
        .expect("verify_block returns a verdict")
}

fn admit(mempool: &mut Mempool, tx: Transaction, tip: u64) -> Result<(), String> {
    mempool
        .add_transaction_with_signing_keys_at_height(tx, None, Some(tip))
        .map_err(|e| e.to_string())
}

#[test]
fn binding_predicate_matches_the_issue_probe() {
    let victim_key = key(1);
    let victim = address_of(&victim_key).to_string();

    let bound = forecast_commit(&victim, &victim_key, "bound");
    assert!(bound.forecast_commit_key_bound());

    // #3290 probe: a fresh random key per commit, all naming the victim.
    for i in 0..3 {
        let probe = forecast_commit(&victim, &key(9000 + i), &format!("probe-{i}"));
        assert!(!probe.forecast_commit_key_bound(), "unrelated key {i} must not bind");
    }

    // Display and case variants of the right address are the same 20 bytes.
    let upper = format!("0x{}", victim.trim_start_matches("0x").to_uppercase());
    assert!(forecast_commit(&upper, &victim_key, "upper").forecast_commit_key_bound());

    // A label that is not an address can never be a bound forecaster.
    assert!(!forecast_commit("WRF1Forecast", &victim_key, "label").forecast_commit_key_bound());

    // Sender right, output wrong (and the reverse) are both refused.
    let mut split = forecast_commit(&victim, &victim_key, "split");
    split.outputs[0].address = address_of(&key(2)).to_string();
    assert!(!split.forecast_commit_key_bound());
    let mut split = forecast_commit(&victim, &victim_key, "split-2");
    split.sender_address = address_of(&key(2));
    assert!(!split.forecast_commit_key_bound());

    // Contribution commits are out of scope at every height.
    let mut contribution = forecast_commit(&victim, &key(3), "contribution");
    contribution.tx_type = TransactionType::ContributionCommit;
    assert!(!contribution.forecast_commit_key_bound());
}

#[test]
fn feature_is_registered_but_unscheduled_and_not_live_schedulable() {
    let feature = ConsensusFeature::ForecasterKeyBinding;
    assert_eq!(ConsensusFeature::parse(FEATURE), Some(feature));
    assert_eq!(ConsensusFeature::ALL.last(), Some(&feature), "appended LAST");
    assert!(
        wrfcoin_shared_protocol::crypto::protocol_support::KNOWN_FEATURES.contains(&FEATURE)
    );
    assert!(!feature.schedulable(None));
    assert!(!feature.schedulable(Some("synthetic-genesis")));
    assert!(feature.has_behaviour());
    for (index, f) in ConsensusFeature::ALL.iter().enumerate() {
        assert_eq!(bincode::serialize(f).unwrap(), (index as u32).to_le_bytes());
    }
}

/// Gate OFF (nothing scheduled in this process): the probe commit is admitted
/// at every height, a block carrying it verifies, and extraction registers it
/// — `verify_commit_authorization_at_height` is `verify_commit_authorization`.
#[tokio::test]
async fn gate_off_unbound_commit_behaves_as_on_main() {
    for height in [1, H, 1_000_000] {
        assert!(!feature_active(ConsensusFeature::ForecasterKeyBinding, height));
    }
    let victim = address_of(&key(1)).to_string();
    let probe = forecast_commit(&victim, &key(9000), "off-probe");
    for height in [1, H, 1_000_000] {
        assert_eq!(
            probe.verify_commit_authorization_at_height(height),
            probe.verify_commit_authorization()
        );
    }
    let mut mempool = Mempool::new(&Config::default());
    admit(&mut mempool, probe.clone(), H).expect("gate off: admitted");
    assert!(verify_block_with(probe.clone(), H).await, "gate off: block accepted");

    let block = Block::new(3, Hash::default(), vec![probe.clone()], 1, H);
    let registered = extract_transitions(&State::new(), &block, H, 0)
        .into_iter()
        .any(|t| matches!(t, StateTransition::RegisterCommit { payload, .. } if payload.commit_id == commit_id(&probe)));
    assert!(registered, "gate off: extraction registers the commit");
}

#[tokio::test]
async fn forecaster_key_binding_refuses_unbound_commits_from_activation() {
    if std::env::var_os(CHILD_ENV).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_ENV, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.contains("1 passed"), "the child must run exactly this test: {stdout}");
        return;
    }

    refresh_feature_schedule_extensions(vec![(FEATURE.to_string(), H)]);
    assert!(!feature_active(ConsensusFeature::ForecasterKeyBinding, H - 1));
    assert!(feature_active(ConsensusFeature::ForecasterKeyBinding, H));

    let victim_key = key(1);
    let victim = address_of(&victim_key).to_string();
    let attacker = key(9000);

    // ── Block validity (sync verifier), H − 1 vs H ─────────────────────────
    assert!(
        verify_block_with(forecast_commit(&victim, &attacker, "v-pre"), H - 1).await,
        "below H an unbound commit leaves the block valid (historical rule)"
    );
    assert!(
        !verify_block_with(forecast_commit(&victim, &attacker, "v-at"), H).await,
        "from H an unbound commit invalidates the block"
    );
    assert!(
        !verify_block_with(forecast_commit(&victim, &attacker, "v-post"), H + 1).await,
        "and stays invalid above H"
    );
    assert!(
        verify_block_with(forecast_commit(&victim, &victim_key, "v-bound"), H).await,
        "control: a bound commit leaves the block valid at H"
    );

    // ── Chain: producer / applier / WAL across H ────────────────────────────
    let mut genesis = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    genesis
        .metadata
        .insert(wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_CHAIN_ID_KEY.into(), "777".into());
    let genesis_transitions = extract_transitions(&State::new(), &genesis, 1, 0);
    let mut applier = State::new();
    applier
        .apply_transitions_batch_at_height(1, genesis_transitions.clone())
        .unwrap();
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    {
        let mut c = chain.write().await;
        c.height = 1;
        c.state = applier.clone();
    }
    let db = std::env::temp_dir().join(format!(
        "forecaster-key-binding-3290-{}.redb",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&db);
    let mut storage = Storage::open(&db, DEFAULT_CHECKPOINT_INTERVAL).unwrap();
    storage.persist_block(genesis, genesis_transitions, None).await.unwrap();

    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let miner = Address([0x41; 20]).to_string();

    // Height H − 1 (tip 1): the probe is admitted and mined, as on main.
    let pre_probe = forecast_commit(&victim, &attacker, "pre-probe");
    let pre_bound = forecast_commit(&victim, &victim_key, "pre-bound");
    admit(&mut *mempool.write().await, pre_probe.clone(), 1).expect("below H: probe admitted");
    admit(&mut *mempool.write().await, pre_bound.clone(), 1).expect("below H: bound admitted");

    // A probe that slipped in under the old rule (admitted against a stale tip
    // 1) must still be dropped by the producer at H: admission at H − 1 gives
    // no right to inclusion at H. Inserted after H − 1 is mined, below.
    let held_probe = forecast_commit(&victim, &attacker, "held-probe");

    let mut expected_commits: Vec<String> = Vec::new();
    for height in [H - 1, H, H + 1] {
        if height == H {
            let at_probe = forecast_commit(&victim, &key(9001), "at-probe");
            let err = admit(&mut *mempool.write().await, at_probe, H - 1)
                .expect_err("from H: probe refused at admission");
            assert!(err.contains("SP_FORECASTER_KEY_UNBOUND"), "{err}");
            let bound = forecast_commit(&victim, &victim_key, "at-bound");
            admit(&mut *mempool.write().await, bound.clone(), H - 1).expect("from H: bound admitted");
            admit(&mut *mempool.write().await, held_probe.clone(), 1)
                .expect("stale-tip admission uses the old rule");
            expected_commits.push(commit_id(&bound));
        }
        if height == H + 1 {
            // Unknown height while the feature is scheduled: fail closed.
            let err = mempool
                .write()
                .await
                .add_transaction_with_signing_keys(forecast_commit(&victim, &key(9002), "gossip"), None)
                .expect_err("scheduled + unknown height: probe refused");
            assert!(err.to_string().contains("SP_FORECASTER_KEY_UNBOUND"), "{err}");
        }

        let before = applier.clone();
        let block = pop
            .create_next_block(chain.clone(), mempool.clone(), &miner)
            .await
            .expect("the real producer builds the block");
        assert_eq!(block.height, height);
        let ids: Vec<String> = block
            .transactions
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::ForecastCommit)
            .map(commit_id)
            .collect();
        if height == H - 1 {
            assert!(ids.contains(&commit_id(&pre_probe)), "below H the probe is mined");
            assert!(ids.contains(&commit_id(&pre_bound)));
            expected_commits.push(commit_id(&pre_probe));
            expected_commits.push(commit_id(&pre_bound));
        } else {
            assert!(
                !ids.contains(&commit_id(&held_probe)),
                "the producer drops a stale-admitted probe at {height}"
            );
        }
        for tx in block.transactions.iter().filter(|tx| tx.tx_type == TransactionType::ForecastCommit) {
            assert!(tx.verify_commit_authorization_at_height(height));
        }
        let producer = chain.read().await.state.clone();

        let transitions = extract_transitions(&before, &block, block.height, 0);
        applier
            .apply_transitions_batch_at_height(block.height, transitions.clone())
            .unwrap();
        storage.persist_block(block, transitions, None).await.unwrap();
        assert_eq!(applier.state_root, producer.state_root, "applier root at {height}");
        assert_eq!(
            applier.pending_commits.keys().collect::<std::collections::BTreeSet<_>>(),
            producer.pending_commits.keys().collect::<std::collections::BTreeSet<_>>(),
            "applier commits at {height}"
        );
    }
    let producer = chain.read().await.state.clone();
    for id in &expected_commits {
        assert!(producer.pending_commits.contains_key(id), "commit {id} registered");
    }
    assert!(!producer.pending_commits.contains_key(&commit_id(&held_probe)));

    let rows = storage.load_wal_transitions_range(1, H + 1).unwrap();
    let mut wal = State::new();
    for height in 1..=H + 1 {
        wal.apply_transitions_batch_at_height(
            height,
            rows.iter().filter(|(h, _)| *h == height).map(|(_, t)| t.clone()),
        )
        .unwrap();
    }
    assert_eq!(wal.state_root, producer.state_root, "WAL replay root");
    assert_eq!(
        wal.pending_commits.keys().collect::<std::collections::BTreeSet<_>>(),
        producer.pending_commits.keys().collect::<std::collections::BTreeSet<_>>(),
        "WAL replay commits"
    );

    drop(storage);
    std::fs::remove_file(db).unwrap();
}
