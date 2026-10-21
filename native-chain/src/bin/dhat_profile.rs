// native-chain/src/bin/dhat_profile.rs
//
// Heap allocation profiler for hot paths.
//
// Usage:
//   cargo run --profile dhat --bin dhat_profile --features dhat-heap
//
// Then open dhat-heap.json at: https://nnethercote.github.io/dh_view/dh_view.html
// and inspect "Total bytes allocated" and "Peak heap bytes".

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    #[cfg(not(feature = "dhat-heap"))]
    {
        eprintln!("dhat heap profiling is disabled.");
        eprintln!("Run with --features dhat-heap to enable:");
        eprintln!("  cargo run --profile dhat --bin dhat_profile --features dhat-heap");
        return;
    }

    #[cfg(feature = "dhat-heap")]
    {
        let _profiler = dhat::Profiler::new_heap();

        // Exercise the hot allocation paths.
        // The profiler measures all heap allocations until it drops at the end
        // of this block, writing dhat-heap.json to the current directory.
        exercise_mempool_fill_and_drain();
        exercise_mempool_rolling();

        eprintln!("dhat-heap.json written. Open in the dh_view viewer to inspect results.");
    }
}

/// Fills a Mempool with 500 transactions, then drains it 100 times.
/// This exercises the clone-heavy retrieval path that Task 8 optimises.
#[allow(dead_code)]
fn exercise_mempool_fill_and_drain() {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        Transaction, TransactionInput, TransactionOutput,
    };
    use wrfcoin_shared_protocol::config::settings::Config;
    use wrfcoin_shared_protocol::crypto::hash::hash_string;
    use wrfcoin_shared_protocol::mempool::pool::Mempool;

    let cfg = Config::default();
    let mut mp = Mempool::new(&cfg);

    for i in 0..500_usize {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("dhat-fill-input-{i:06}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1_000,
                address: format!("wrf-dhat-{i}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.fee = (i as u64 % 100) + 1;
        tx.set_id();
        let _ = mp.add_transaction(tx);
    }

    // Drain many times to amplify alloc measurement
    for _ in 0..100 {
        let _ = mp.get_transactions_with_limit(64 * 1024);
    }
}

/// Exercises rolling add+drain to profile mempool under sustained load.
#[allow(dead_code)]
fn exercise_mempool_rolling() {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        Transaction, TransactionInput, TransactionOutput,
    };
    use wrfcoin_shared_protocol::config::settings::Config;
    use wrfcoin_shared_protocol::crypto::hash::hash_string;
    use wrfcoin_shared_protocol::mempool::pool::Mempool;

    let cfg = Config::default();
    let mut mp = Mempool::new(&cfg);

    for round in 0..50_usize {
        for i in 0..20_usize {
            let tag = round * 20 + i;
            let mut tx = Transaction::new(
                vec![TransactionInput {
                    txid: hash_string(&format!("dhat-roll-input-{tag:06}")),
                    vout: 0,
                    signature: None,
                    public_key: None,
                    coinbase_data: None,
                }],
                vec![TransactionOutput {
                    amount: 500,
                    address: format!("wrf-roll-{tag}"),
                    script: "OP_TRUE".to_string(),
                    nft_data: None,
                }],
            );
            tx.fee = (tag as u64 % 50) + 1;
            tx.set_id();
            let _ = mp.add_transaction(tx);
        }
        let _ = mp.get_transactions_with_limit(16 * 1024);
    }
}
