use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionInput, TransactionOutput},
    config::settings::Config,
    crypto::hash::hash_string,
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

fn build_mempool(tx_count: usize) -> Mempool {
    let cfg = Config::default();
    let mut mempool = Mempool::new(&cfg);
    for i in 0..tx_count {
        let fee = ((i % 10) as u64 + 1) * 1_000;
        mempool
            .add_transaction(make_test_tx(&format!("{i}"), fee))
            .expect("benchmark mempool should accept synthetic transactions");
    }
    mempool
}

fn bench_mempool_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_insert");
    for tx_count in [100usize, 500, 1_000] {
        group.bench_with_input(
            BenchmarkId::new("tx_count", tx_count),
            &tx_count,
            |b, &count| {
                b.iter_batched(
                    Config::default,
                    |cfg| {
                        let mut mempool = Mempool::new(&cfg);
                        for i in 0..count {
                            mempool
                                .add_transaction(make_test_tx(
                                    &format!("{i}"),
                                    (i as u64 + 1) * 500,
                                ))
                                .expect("benchmark insert should succeed");
                        }
                        black_box(mempool);
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();
}

fn bench_mempool_get_transaction(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_get_transaction");
    for tx_count in [100usize, 500, 1_000] {
        group.bench_with_input(
            BenchmarkId::new("pool_size", tx_count),
            &tx_count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let mempool = build_mempool(count);
                        let target = make_test_tx("lookup-target", 15_000);
                        let tx_id = target.id.expect("synthetic tx should have id");
                        let mut mempool = mempool;
                        mempool
                            .add_transaction(target)
                            .expect("lookup target should insert");
                        (mempool, tx_id)
                    },
                    |(mempool, tx_id)| {
                        let tx = mempool.get_transaction(&tx_id);
                        black_box(tx);
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();
}

fn bench_mempool_get_with_limit(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_get_with_limit");
    for tx_count in [100usize, 500, 1_000] {
        group.bench_with_input(
            BenchmarkId::new("pool_size", tx_count),
            &tx_count,
            |b, &count| {
                b.iter_batched(
                    || build_mempool(count),
                    |mempool| {
                        let selected = mempool.get_transactions_with_limit(50 * 1024);
                        black_box(selected.len());
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_mempool_insert,
    bench_mempool_get_transaction,
    bench_mempool_get_with_limit,
);
criterion_main!(benches);
