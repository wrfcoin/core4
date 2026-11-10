//! Forecast late score (`forecast_late_score`): producer / follower / WAL
//! parity through the real block producer, over a whole lifecycle.
//!
//! One child process (the feature schedule is process-wide) with
//! `forecaster_key_binding` and `forecast_late_score` both scheduled at 0, so
//! the lane is dormant for heights 2..=1999 and in force from 2000.
//!
//! - Heights 2..=2000: empty blocks. The lane state stays empty.
//! - Then, on a manual clock: a station reading lands and reaches quorum; two
//!   forecasts are committed and revealed; the truth reading lands near the
//!   target; a `ForecastScore` transaction scores both forecasts at
//!   target + 35 min; the bucket closes a day later and pays.
//!
//! At EVERY height the block the producer built (`create_next_block`) is
//! verified by a separate follower (`verify_block`), applied by the follower
//! through `extract_transitions` → `apply_transitions_batch_at_height`, and
//! persisted. The follower's `state_root`, lane state and balances must equal
//! the producer's own post-state. At the end the WAL is replayed from an empty
//! state and must reach the same root and lane state.

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
use std::sync::atomic::{AtomicI64, Ordering};

use rustc_hash::FxHashSet;
use tokio::sync::RwLock;
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::consensus::proof_of_publishing::{Clock, ProofOfPublishing};
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, ForecastArtifactManifest, OracleAttestationPayload, RevealPayload, Transaction,
    TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::forecast_late_score::{
    ForecastScorePayload, LateScoreVariable, ObservationAttestation, ObservationSignature,
    SCORE_OPENS_AFTER_SECS, bucket_closes_at, bucket_hour, observation_attestation_message,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS, GENESIS_CHAIN_ID_KEY,
    forecast_late_score_active, refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};

const ACTIVE: u64 = FORECAST_LATE_SCORE_KEY_BINDING_LEAD_BLOCKS;
const CHILD_ENV: &str = "WRFCOIN_FORECAST_LATE_SCORE_PARITY_CHILD";
const TEST_NAME: &str = "forecast_late_score_producer_follower_wal";
const STATION: &str = "st-parity-1";
/// A whole hour.
const START: i64 = 1_800_000_000 - 1_800_000_000 % 3_600;
const HOUR: i64 = 3_600;

struct ManualClock(AtomicI64);
impl Clock for ManualClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(self.0.load(Ordering::SeqCst), 0).expect("representable")
    }
}

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn address_of(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}
fn oracle_id(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_hex()
}

/// A transaction shell stamped with the manual clock's time, paying its own
/// sender (a forecast commit must be key-bound: sender and every output are
/// the address of the key that signed it).
fn shell(tx_type: TransactionType, sender: Address, now: i64, tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("late-score-parity-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: sender.to_string(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = tx_type;
    tx.sender_address = sender;
    tx.timestamp = chrono::DateTime::from_timestamp(now, 0).expect("representable");
    tx
}

fn with_data<T: serde::Serialize>(mut tx: Transaction, payload: &T, fee: u64) -> Transaction {
    tx.data = Some(serde_json::to_string(payload).expect("payload serializes"));
    tx.fee = fee;
    tx.set_id();
    tx
}

struct Pair {
    commit: Transaction,
    reveal: Transaction,
    commit_id: String,
}

/// A commit and its reveal for `contribution_type`, signed by `signer`.
fn commit_reveal(
    signer: &PrivateKey,
    contribution_type: &str,
    data: serde_json::Value,
    lead_hours: Option<u32>,
    payload_time: i64,
    tag: &str,
) -> Pair {
    let nonce = format!("nonce-{tag}");
    let commit_id =
        Transaction::compute_commit_id(&data, &nonce, STATION, payload_time, contribution_type);
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: STATION.to_string(),
        timestamp: payload_time,
        contribution_type: contribution_type.to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(signer);
    let reveal = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: STATION.to_string(),
        timestamp: payload_time,
        contribution_type: contribution_type.to_string(),
        data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: lead_hours.map(|horizon_hours| ForecastArtifactManifest {
            schema_version: 1,
            model_id: "parity-model".to_string(),
            input_hash: "11".repeat(32),
            output_hash: "22".repeat(32),
            produced_at: payload_time,
            region: STATION.to_string(),
            horizon_hours,
            input_variables: vec!["temp_c".to_string()],
            output_variables: vec!["temp_c".to_string()],
        }),
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };
    let (commit_type, reveal_type) = if contribution_type == "forecast" {
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
    let sender = address_of(signer);
    Pair {
        commit: with_data(
            shell(commit_type, sender, payload_time, &format!("commit-{tag}")),
            &commit,
            0,
        ),
        // Non-zero fee: a zero-fee reveal needs a compute receipt.
        reveal: with_data(
            shell(reveal_type, sender, payload_time, &format!("reveal-{tag}")),
            &reveal,
            1,
        ),
        commit_id,
    }
}

fn reading(owner: &PrivateKey, now: i64, tag: &str) -> Pair {
    let data = serde_json::json!({
        "quality": 1.0, "volume": 1.0, "uniqueness": 1.0,
        "temperature_c": 20.0, "humidity_percent": 60.0, "pressure_hpa": 1014.0,
        "wind_speed_ms": 2.8, "precipitation_mm": 0.0, "dewpoint_c": 12.0
    });
    commit_reveal(owner, "contribution", data, None, now, tag)
}

fn attestation_tx(commit_id: &str, oracle: &PrivateKey, now: i64, tag: &str) -> Transaction {
    let mut payload = OracleAttestationPayload {
        commit_id: commit_id.to_string(),
        oracle_id: oracle_id(oracle),
        approved: true,
        signature: None,
        confidence: Some(1.0),
        timestamp: now,
        nonce: Some(format!("att-{tag}")),
    };
    payload.signature = Some(
        oracle
            .sign(payload.signing_message().as_bytes())
            .to_string(),
    );
    with_data(
        shell(
            TransactionType::OracleAttestation,
            address_of(oracle),
            now,
            &format!("att-{tag}"),
        ),
        &payload,
        0,
    )
}

#[test]
fn forecast_late_score_producer_follower_wal() {
    if std::env::var_os(CHILD_ENV).is_none() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_ENV, "1")
            .env("WRFCOIN_CHAIN_ID", "777")
            .env("WRFCOIN_NETWORK_ID", "testnet")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "child failed:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    refresh_feature_schedule_extensions(vec![
        ("forecaster_key_binding".to_string(), 0),
        ("forecast_late_score".to_string(), 0),
    ]);
    assert!(!forecast_late_score_active(ACTIVE - 1));
    assert!(forecast_late_score_active(ACTIVE));
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run());
}

async fn run() {
    let owner = key(0x01);
    let (alice, bob) = (key(0x11), key(0x12));
    let (oracle_a, oracle_b) = (key(0x21), key(0x22));
    let oracle_set: FxHashSet<String> = [oracle_id(&oracle_a), oracle_id(&oracle_b)]
        .into_iter()
        .collect();

    // Genesis, with the station and the oracle set as WAL transitions.
    let mut genesis = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    genesis
        .metadata
        .insert(GENESIS_CHAIN_ID_KEY.into(), "777".into());
    let mut genesis_transitions = extract_transitions(&State::new(), &genesis, 1, 0);
    genesis_transitions.push(StateTransition::ApplyStationRegistration {
        station: WeatherStation {
            station_id: STATION.to_string(),
            owner_address: address_of(&owner).to_string(),
            location: StationLocation {
                latitude: 46.7867,
                longitude: -92.1005,
                elevation_meters: Some(215.0),
            },
            hardware_model: "parity-station".to_string(),
            calibration_date: None,
            is_active: true,
            reputation_score: 50,
            data_quality_tier: DataQualityTier::Gold,
            // Not `Real`: a real reading needs same-block quorum to be mined,
            // which is a different fence from the one under test. An
            // unclassified reading counts for the skill rule.
            provenance_class: ContributionProvenanceClass::Unclassified,
            ring_public_key: None,
        },
    });
    genesis_transitions.push(StateTransition::SetOracleConfig {
        oracle_set: oracle_set.iter().cloned().collect(),
        oracle_quorum: 2,
    });
    let mut follower = State::new();
    follower
        .apply_transitions_batch_at_height(1, genesis_transitions.clone())
        .unwrap();
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    {
        let mut c = chain.write().await;
        c.height = 1;
        c.state = follower.clone();
    }
    let db = std::env::temp_dir().join(format!(
        "forecast-late-score-parity-{}.redb",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&db);
    // No checkpoint inside the run, so the WAL keeps every block for the replay.
    let mut storage = Storage::open(&db, 1_000_000).unwrap();
    storage
        .persist_block(genesis, genesis_transitions, None)
        .await
        .unwrap();

    let clock = Arc::new(ManualClock(AtomicI64::new(START)));
    let mut producer_pop = ProofOfPublishing::new(1, 60, 0, 32);
    producer_pop.declare_no_genesis_oracles();
    producer_pop.set_clock(clock.clone());
    let mut verifier_pop = ProofOfPublishing::new(1, 60, 0, 32);
    verifier_pop.declare_no_genesis_oracles();
    verifier_pop.set_clock(clock.clone());
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let miner = Address([0x41; 20]).to_string();

    // One block: admit `txs`, produce, verify as a follower, apply as a
    // follower, persist, compare. Returns the block's own timestamp.
    let mut height = 1u64;
    macro_rules! block {
        ($now:expr, $txs:expr) => {{
            height += 1;
            let now: i64 = $now;
            clock.0.store(now, Ordering::SeqCst);
            let txs: Vec<(&str, Transaction)> = $txs;
            let carried = txs.len();
            for (tag, tx) in txs {
                let signing_keys = chain.read().await.state.signing_keys.clone();
                mempool
                    .write()
                    .await
                    .add_transaction_with_signing_keys_at_height(
                        tx,
                        Some(&signing_keys),
                        Some(height - 1),
                    )
                    .unwrap_or_else(|e| {
                        panic!("{tag} refused at admission for height {height}: {e}")
                    });
            }
            let before = follower.clone();
            let block = producer_pop
                .create_next_block(chain.clone(), mempool.clone(), &miner)
                .await
                .unwrap_or_else(|e| panic!("producer failed at {height}: {e}"));
            assert_eq!(block.height, height);
            // Coinbase plus every transaction offered: nothing was dropped.
            assert_eq!(
                block.transactions.len(),
                carried + 1,
                "the producer dropped a transaction at {height}"
            );
            let block_time = block.timestamp.timestamp();
            assert!(
                verifier_pop
                    .verify_block(&block, &oracle_set, 2, &[])
                    .await
                    .expect("verdict"),
                "a follower rejects the producer's block at {height}"
            );
            let producer = chain.read().await.state.clone();
            let transitions = extract_transitions(&before, &block, block.height, block_time);
            follower
                .apply_transitions_batch_at_height(block.height, transitions.clone())
                .unwrap_or_else(|e| panic!("follower apply failed at {height}: {e}"));
            storage
                .persist_block(block, transitions, None)
                .await
                .unwrap();
            assert_eq!(
                follower.forecast_late_score, producer.forecast_late_score,
                "lane state at {height}"
            );
            assert_eq!(
                follower.state_root, producer.state_root,
                "follower root at {height}"
            );
            for who in [&alice, &bob, &owner] {
                let balance = |s: &State| s.get_account(&address_of(who)).map(|a| a.balance);
                assert_eq!(
                    balance(&follower),
                    balance(&producer),
                    "balance at {height}"
                );
            }
            mempool.write().await.clear_all();
            block_time
        }};
    }

    // ---- Dormant: empty blocks up to the effective height. ----
    let mut now = START;
    while height < ACTIVE {
        now += 60;
        block!(now, Vec::new());
        assert!(
            follower.forecast_late_score.is_empty(),
            "the lane is empty at {height}"
        );
    }
    assert_eq!(height, ACTIVE);

    // ---- A station reading lands and reaches quorum (the window). ----
    now += 60;
    let window = reading(&owner, now, "window");
    block!(now, vec![("window commit", window.commit.clone())]);
    now += 60;
    block!(
        now,
        vec![
            ("window reveal", window.reveal.clone()),
            (
                "window att a",
                attestation_tx(&window.commit_id, &oracle_a, now, "window-a")
            ),
            (
                "window att b",
                attestation_tx(&window.commit_id, &oracle_b, now, "window-b")
            ),
        ]
    );

    // ---- Two forecasts, 1 h lead, committed in one block. ----
    now += 60;
    let f_alice = commit_reveal(
        &alice,
        "forecast",
        serde_json::json!({"temp_c_centi": 2_040}),
        Some(1),
        now,
        "alice",
    );
    let f_bob = commit_reveal(
        &bob,
        "forecast",
        serde_json::json!({"temp_c_centi": 2_150}),
        Some(1),
        now,
        "bob",
    );
    let commit_time = block!(
        now,
        vec![
            ("alice commit", f_alice.commit.clone()),
            ("bob commit", f_bob.commit.clone())
        ]
    );
    let commit_height = height;
    assert_eq!(
        follower.forecast_late_score.pending[&f_alice.commit_id].commit_time,
        Some(commit_time),
        "the end-of-block close stamped the commit block's time"
    );
    now += 60;
    block!(
        now,
        vec![
            ("alice reveal", f_alice.reveal.clone()),
            ("bob reveal", f_bob.reveal.clone())
        ]
    );
    let target = commit_time + HOUR;
    {
        let lane = &follower.forecast_late_score;
        assert_eq!(lane.records.len(), 2);
        let record = &lane.records[&f_alice.commit_id];
        assert_eq!(record.forecaster, address_of(&alice));
        assert_eq!(
            (record.commit_height, record.commit_time, record.target),
            (commit_height, commit_time, target)
        );
        assert!(lane.pending.is_empty());
    }

    // ---- The truth reading lands near the target. ----
    now = target - 60;
    let truth = reading(&owner, now, "truth");
    block!(now, vec![("truth commit", truth.commit.clone())]);
    now = target;
    block!(
        now,
        vec![
            ("truth reveal", truth.reveal.clone()),
            (
                "truth att a",
                attestation_tx(&truth.commit_id, &oracle_a, now, "truth-a")
            ),
            (
                "truth att b",
                attestation_tx(&truth.commit_id, &oracle_b, now, "truth-b")
            ),
        ]
    );

    // ---- Score at target + 35 min. ----
    let attestation = ObservationAttestation {
        station_id: STATION.to_string(),
        variable: LateScoreVariable::TemperatureC,
        lead_hours: 1,
        commit_height,
        commit_time,
        target,
        truth: 2_000,
        truth_commit_id: truth.commit_id.clone(),
        reference: 300,
        mean24: 1_900,
        last: 2_100,
        window_hash: "5a".repeat(32),
        window_commit_ids: vec![window.commit_id.clone()],
    };
    let identity = ChainIdentity::active();
    let message =
        observation_attestation_message(&identity.chain_id, &identity.network_id, &attestation);
    let mut signatures: Vec<ObservationSignature> = [&oracle_a, &oracle_b]
        .into_iter()
        .map(|oracle| ObservationSignature {
            oracle: oracle_id(oracle),
            ed25519: oracle.sign(&message).to_hex(),
        })
        .collect();
    signatures.sort_by(|a, b| a.oracle.cmp(&b.oracle));
    let mut forecast_commit_ids = vec![f_alice.commit_id.clone(), f_bob.commit_id.clone()];
    forecast_commit_ids.sort();
    let payload = ForecastScorePayload {
        attestation,
        signatures,
        forecast_commit_ids,
    };
    now = target + SCORE_OPENS_AFTER_SECS;
    let relay = Address([0x77; 20]);
    block!(
        now,
        vec![(
            "score",
            with_data(
                shell(TransactionType::ForecastScore, relay, now, "score"),
                &payload,
                0
            )
        )]
    );
    {
        let lane = &follower.forecast_late_score;
        assert_eq!(lane.records[&f_alice.commit_id].skill, Some(866_667));
        assert_eq!(lane.records[&f_bob.commit_id].skill, Some(500_000));
    }

    // ---- The bucket closes a day after the end of its hour, and pays. ----
    let closes_at = bucket_closes_at(bucket_hour(target).unwrap()).unwrap();
    now = closes_at - 60;
    block!(now, Vec::new());
    assert_eq!(
        follower.forecast_late_score.buckets.len(),
        1,
        "not closed one minute early"
    );
    let before_close = |who: &PrivateKey, s: &State| {
        s.get_account(&address_of(who))
            .map_or(0, |a| a.balance.into_inner())
    };
    let (alice_before, bob_before) = (
        before_close(&alice, &follower),
        before_close(&bob, &follower),
    );
    now = closes_at;
    block!(now, Vec::new());
    {
        let lane = &follower.forecast_late_score;
        assert!(
            lane.buckets.is_empty() && lane.records.is_empty(),
            "closed and deleted"
        );
        assert_eq!(lane.pool.len(), 1, "the epoch ledger records the payments");
        assert_eq!(before_close(&alice, &follower) - alice_before, 86_666_700);
        assert_eq!(before_close(&bob, &follower) - bob_before, 50_000_000);
        assert_eq!(
            lane.pool.values().next().unwrap().paid.into_inner(),
            136_666_700
        );
    }
    now += 60;
    block!(now, Vec::new());
    let last = height;
    let producer = chain.read().await.state.clone();

    // ---- WAL replay from an empty state. ----
    let rows = storage.load_wal_transitions_range(1, last).unwrap();
    let mut wal = State::new();
    for h in 1..=last {
        wal.apply_transitions_batch_at_height(
            h,
            rows.iter()
                .filter(|(height, _)| *height == h)
                .map(|(_, t)| t.clone()),
        )
        .unwrap_or_else(|e| panic!("WAL replay failed at {h}: {e}"));
    }
    assert_eq!(wal.state_root, producer.state_root, "WAL replay root");
    assert_eq!(
        wal.forecast_late_score, producer.forecast_late_score,
        "WAL replay lane state"
    );

    drop(storage);
    std::fs::remove_file(db).unwrap();
}
