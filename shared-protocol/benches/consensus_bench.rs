// shared-protocol/benches/consensus_bench.rs
//
// Criterion benchmarks for hot consensus and mempool paths.
// Run with: cargo bench -p wrfcoin-shared-protocol --bench consensus_bench

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use rand::rngs::OsRng;
use rustc_hash::FxHashSet;
use serde_json::json;
use wrfcoin_forecast_score::Pair;
use wrfcoin_shared_protocol::{
    blockchain::{
        block::Block,
        state::{Account, PopLivenessPolicy, State},
        transaction::{
            CommitPayload, ForecastAccuracyMetrics, ForecastArtifactManifest,
            ForecastVerificationReceipt, RevealPayload, Transaction, TransactionInput,
            TransactionOutput, TransactionType,
        },
    },
    common::{
        constants::DEFAULT_ORACLE_QUORUM,
        types::{Address, Balance},
    },
    config::settings::Config,
    consensus::proof_of_publishing::ProofOfPublishing,
    crypto::{
        hash::{Hash, hash_string},
        signature::{PrivateKey, PublicKey},
    },
    mempool::pool::Mempool,
};

fn make_test_tx(tag: &str, fee: u64) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("bench-input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1_000,
            address: format!("wrf-bench-{tag}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.fee = fee;
    tx.set_id();
    tx
}

fn build_mempool(n: usize) -> Mempool {
    let cfg = Config::default();
    let mut mp = Mempool::new(&cfg);
    for i in 0..n {
        let fee = ((i % 10) as u64 + 1) * 1_000;
        let _ = mp.add_transaction(make_test_tx(&format!("{i}"), fee));
    }
    mp
}

fn make_validation_block(height: u64) -> Block {
    let mut block = Block::new(3, Hash::default(), vec![], 1, height);
    block
        .metadata
        .insert("publishing_score".to_string(), "0".to_string());
    block.metadata.insert(
        "useful_work_liveness_fallback".to_string(),
        "empty_liveness".to_string(),
    );
    block
}

fn make_forecast_commit_and_reveal(
    index: usize,
    oracle_private_key: &PrivateKey,
    oracle_id_hex: &str,
) -> (Transaction, Transaction) {
    let station_id = format!("station-{index}");
    let timestamp = 1_700_000_000 + index as i64;
    let nonce = format!("nonce-{index}");
    let data = json!({
        "schema_version": "wrf.weather.v1",
        "observation": {
            "temperature_c": 20.0 + (index % 5) as f64,
            "humidity_pct": 55.0,
            "pressure_hpa": 1012.0,
            "wind_speed_mps": 3.2,
            "wind_direction_deg": 180.0,
            "precipitation_mm": 0.0
        }
    });
    let contribution_type = "forecast";
    let commit_id =
        Transaction::compute_commit_id(&data, &nonce, &station_id, timestamp, contribution_type);

    let manifest = ForecastArtifactManifest {
        schema_version: 1,
        model_id: "wrf-v1".to_string(),
        input_hash: "aa".repeat(32),
        output_hash: "bb".repeat(32),
        produced_at: timestamp,
        region: "global".to_string(),
        horizon_hours: 6,
        input_variables: vec![
            "humidity".to_string(),
            "temperature".to_string(),
            "wind".to_string(),
        ],
        output_variables: vec!["precipitation".to_string(), "temperature".to_string()],
    };
    let manifest_digest = manifest.manifest_digest();

    let pairs = vec![
        Pair {
            predicted: 20.0,
            observed: 19.8,
        },
        Pair {
            predicted: 21.0,
            observed: 21.3,
        },
        Pair {
            predicted: 22.0,
            observed: 22.2,
        },
        Pair {
            predicted: 23.0,
            observed: 22.9,
        },
    ];
    let metrics = ForecastAccuracyMetrics::from_pairs(&pairs);

    let mut receipt = ForecastVerificationReceipt {
        manifest_digest,
        commit_id: commit_id.clone(),
        oracle_id: oracle_id_hex.to_string(),
        metrics,
        verified_at: timestamp + 60,
        nonce: format!("receipt-{index}"),
        zk_proof: None,
        signature: None,
    };
    receipt.signature = Some(
        oracle_private_key
            .sign(receipt.signing_message().as_bytes())
            .to_hex(),
    );

    let commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.clone(),
        timestamp,
        contribution_type: contribution_type.to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    let reveal_payload = RevealPayload {
        commit_id,
        station_id,
        timestamp,
        contribution_type: contribution_type.to_string(),
        data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: Some(manifest),
        verification_receipt: Some(receipt),
        compute_receipt: None,
        quality_tier: None,
    };

    let mut commit_tx = make_test_tx(&format!("commit-{index}"), 100);
    commit_tx.tx_type = TransactionType::ForecastCommit;
    commit_tx.data = Some(serde_json::to_string(&commit_payload).expect("commit payload"));
    commit_tx.set_id();

    let mut reveal_tx = make_test_tx(&format!("reveal-{index}"), 100);
    reveal_tx.tx_type = TransactionType::ForecastReveal;
    reveal_tx.data = Some(serde_json::to_string(&reveal_payload).expect("reveal payload"));
    reveal_tx.set_id();

    (commit_tx, reveal_tx)
}

/// Build a test block at `height` containing `n` weather-data transactions
/// (forecast commit/reveal pairs).  The block includes a coinbase transaction
/// plus the weather data, making it suitable for `verify_block` benchmarks.
fn build_test_block(n: usize, height: u64) -> (Block, FxHashSet<String>) {
    let oracle_private_key =
        PrivateKey::from_bytes(&[7u8; ed25519_dalek::SECRET_KEY_LENGTH]).expect("oracle key");
    let oracle_id_hex = PublicKey::from_private_key(&oracle_private_key).to_hex();

    let mut txs = Vec::with_capacity(1 + 2 * n);

    // Coinbase transaction (index 0).
    let coinbase_reward =
        wrfcoin_shared_protocol::consensus::proof_of_publishing::adjusted_block_reward(height);
    let mut coinbase = Transaction::new(
        vec![TransactionInput {
            txid: Hash::default(),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: Some(format!("bench-coinbase-{height}")),
        }],
        vec![TransactionOutput {
            amount: coinbase_reward,
            address: "wrf-bench-miner".to_string(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    coinbase.tx_type = TransactionType::Coinbase;
    coinbase.set_id();
    txs.push(coinbase);

    // Weather data pairs: commit + reveal for each station.
    for i in 0..n {
        let (commit_tx, reveal_tx) =
            make_forecast_commit_and_reveal(i, &oracle_private_key, &oracle_id_hex);
        txs.push(commit_tx);
        txs.push(reveal_tx);
    }

    let mut block = Block::new(3, Hash::default(), txs, 1, height);
    block
        .metadata
        .insert("publishing_score".to_string(), "100".to_string());

    let mut oracle_set = FxHashSet::default();
    oracle_set.insert(oracle_id_hex);
    (block, oracle_set)
}

fn build_state_with_accounts(account_count: usize) -> State {
    let mut state = State::new();
    for i in 0..account_count {
        let mut bytes = [0u8; 20];
        bytes[..8].copy_from_slice(&(i as u64).to_le_bytes());
        state
            .accounts
            .insert(Address::new(bytes), Account::new(Balance::new(1_000)));
    }
    state
}

fn bench_mempool_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_add");
    for n in [100usize, 500, 1_000] {
        group.bench_with_input(BenchmarkId::new("tx_count", n), &n, |b, &count| {
            b.iter_batched(
                Config::default,
                |cfg| {
                    let mut mp = Mempool::new(&cfg);
                    for i in 0..count {
                        let _ =
                            mp.add_transaction(make_test_tx(&format!("{i}"), (i as u64 + 1) * 500));
                    }
                    black_box(mp);
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_mempool_get_with_limit(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_get_with_limit");
    for n in [100usize, 500, 1_000] {
        group.bench_with_input(BenchmarkId::new("pool_size", n), &n, |b, &count| {
            b.iter_batched(
                || build_mempool(count),
                |mp| {
                    let selected = mp.get_transactions_with_limit(50 * 1024);
                    black_box(selected.len());
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_mempool_get_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_get_all");
    for n in [100usize, 500] {
        group.bench_with_input(BenchmarkId::new("pool_size", n), &n, |b, &count| {
            b.iter_batched(
                || build_mempool(count),
                |mp| {
                    let all = mp.get_all_transactions();
                    black_box(all.len());
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_block_validation_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_validation_throughput");
    group.sample_size(10);
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

    group.bench_function("construct_validate_1000_blocks", |b| {
        b.iter_batched(
            || {
                let mut pop = ProofOfPublishing::new(1, 60, 1, 10);
                pop.set_pop_liveness_policy(PopLivenessPolicy {
                    useful_work_activation_height: 0,
                })
                .expect("set liveness policy");
                let oracle_set = FxHashSet::default();
                let blocks = (1..=1000u64).map(make_validation_block).collect::<Vec<_>>();
                (pop, oracle_set, blocks)
            },
            |(mut pop, oracle_set, blocks)| {
                let verified = runtime.block_on(async {
                    let mut verified = 0usize;
                    for block in &blocks {
                        if pop
                            .verify_block(block, &oracle_set, DEFAULT_ORACLE_QUORUM.max(1), &[])
                            .await
                            .expect("verify block")
                        {
                            verified += 1;
                        }
                    }
                    verified
                });
                black_box(verified);
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_tx_sign_and_verify(c: &mut Criterion) {
    let mut csprng = OsRng;
    let private_key = PrivateKey::generate(&mut csprng);
    let public_key = PublicKey::from_private_key(&private_key);
    let payloads = (0..1000usize)
        .map(|i| format!("tx-signature-payload-{i}").into_bytes())
        .collect::<Vec<_>>();

    c.bench_function("tx_sign_and_verify_1000", |b| {
        b.iter(|| {
            let mut verified_count = 0usize;
            for payload in &payloads {
                let signature = private_key.sign(payload);
                if public_key.verify(payload, &signature) {
                    verified_count += 1;
                }
            }
            black_box(verified_count);
        })
    });
}

fn bench_pop_scoring_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("pop_scoring_pipeline");
    group.sample_size(10);

    let oracle_private_key =
        PrivateKey::from_bytes(&[7u8; ed25519_dalek::SECRET_KEY_LENGTH]).expect("oracle key");
    let oracle_id_hex = PublicKey::from_private_key(&oracle_private_key).to_hex();

    group.bench_function("forecast_accuracy_1000_receipts", |b| {
        b.iter_batched(
            || {
                let mut pop = ProofOfPublishing::new(1, 60, 0, 3000);
                let mut oracle_set = FxHashSet::default();
                oracle_set.insert(oracle_id_hex.clone());
                pop.set_authorized_oracles(oracle_set);

                let mut txs = Vec::with_capacity(2000);
                for i in 0..1000usize {
                    let (commit_tx, reveal_tx) =
                        make_forecast_commit_and_reveal(i, &oracle_private_key, &oracle_id_hex);
                    txs.push(commit_tx);
                    txs.push(reveal_tx);
                }
                (pop, txs)
            },
            |(pop, txs)| {
                let score = pop.calculate_block_publishing_score(&txs, 1);
                black_box(score);
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_state_root_computation(c: &mut Criterion) {
    c.bench_function("state_root_recompute_10k_accounts", |b| {
        b.iter_batched(
            || build_state_with_accounts(10_000),
            |mut state| {
                state.recalculate_state_root();
                black_box(state.state_root);
            },
            BatchSize::LargeInput,
        );
    });
}

fn bench_state_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_clone");
    for n in [1_000usize, 10_000, 100_000] {
        group.bench_with_input(BenchmarkId::new("accounts", n), &n, |b, &count| {
            let state = build_state_with_accounts(count);
            b.iter(|| black_box(state.clone()));
        });
    }
    group.finish();
}

fn bench_weather_block_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("weather_block_validation");
    group.sample_size(10);
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

    for n in [10usize, 50, 100] {
        group.bench_with_input(BenchmarkId::new("weather_txs", n), &n, |b, &count| {
            b.iter_batched(
                || {
                    let mut pop = ProofOfPublishing::new(1, 60, 0, 3000);
                    pop.set_pop_liveness_policy(PopLivenessPolicy {
                        useful_work_activation_height: 0,
                    })
                    .expect("set liveness policy");
                    let (block, oracle_set) = build_test_block(count, 1);
                    pop.set_authorized_oracles(oracle_set.clone());
                    (pop, block, oracle_set)
                },
                |(mut pop, block, oracle_set)| {
                    let result = runtime.block_on(async {
                        pop.verify_block(
                            &block,
                            &oracle_set,
                            DEFAULT_ORACLE_QUORUM.max(1),
                            &[],
                        )
                        .await
                    });
                    let _ = black_box(result);
                },
                BatchSize::LargeInput,
            )
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_mempool_add,
    bench_mempool_get_with_limit,
    bench_mempool_get_all,
    bench_block_validation_throughput,
    bench_weather_block_validation,
    bench_tx_sign_and_verify,
    bench_pop_scoring_pipeline,
    bench_state_root_computation,
    bench_state_clone,
);
criterion_main!(benches);
