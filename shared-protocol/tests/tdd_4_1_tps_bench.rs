// TDD-4.1 (issue #559): TPS benchmark — saturate mempool, drive block production
// via PoW-bypass (difficulty=1), assert TPS >= MIN_TPS and full mempool drain.
//
// Run: cargo test -p wrfcoin-shared-protocol --test tdd_4_1_tps_bench -- --nocapture

use std::time::Instant;
use wrfcoin_shared_protocol::{
    blockchain::{
        block::Block,
        chain::Blockchain,
        transaction::{Transaction, TransactionInput, TransactionOutput},
    },
    config::settings::Config,
    crypto::hash::{Hash, hash_string},
    mempool::pool::Mempool,
};

const TX_COUNT: usize = 2_000;
/// Byte budget passed to `get_transactions_with_limit` — selects txs until total
/// serialized size exceeds this limit. 500 KB fits ~1 500 test txs (~330 bytes each).
const BLOCK_BYTE_BUDGET: usize = 500 * 1024; // 500 KB
const MIN_TPS: f64 = 500.0;

fn make_transfer_tx(tag: &str, index: usize, fee: u64) -> Transaction {
    // Pattern from shared-protocol/src/mempool/pool.rs lines 726-744
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("tps-input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1_000,
            address: format!("wrf-tps-{tag}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    // Distinct sender per tx: #2061's per-sender admission cap (default 1024)
    // keys on sender_address, and a single sender saturating the pool is
    // exactly what that cap exists to stop. This bench models many senders.
    let mut sender = [0u8; 20];
    sender[..8].copy_from_slice(&(index as u64).to_be_bytes());
    tx.sender_address = wrfcoin_shared_protocol::common::types::Address(sender);
    tx.fee = fee;
    tx.set_id();
    tx
}

/// Build a block bypassing PoW: difficulty=1, then immediately calculate_hash + set_hash.
/// This tests state-machine throughput, not mining speed.
fn mine_block_bypass_pow(prev_hash: Hash, txns: Vec<Transaction>, height: u64) -> Block {
    let mut block = Block::new(1, prev_hash, txns, 1 /* difficulty=1 */, height);
    let h = block.calculate_hash();
    block.set_hash(h);
    block
}

/// TDD-4.1: Saturate mempool with 2,000 synthetic transactions, drive block
/// production, assert TPS >= 500 and mempool drains to zero.
#[test]
fn tdd_4_1_tps_and_mempool_drain() {
    let cfg = Config::default();
    let mut mempool = Mempool::new(&cfg);
    let mut blockchain = Blockchain::default();

    // Phase 1: saturate mempool
    let mut added = 0usize;
    for i in 0..TX_COUNT {
        let fee = ((i % 20) as u64 + 1) * 1_000;
        if mempool.add_transaction(make_transfer_tx(&format!("{i}"), i, fee)).is_ok() {
            added += 1;
        }
    }
    assert_eq!(added, TX_COUNT, "all {TX_COUNT} transactions must be accepted by mempool");
    assert_eq!(mempool.size(), TX_COUNT, "mempool.size() must equal TX_COUNT");

    // Phase 2: drive block production via PoW bypass
    let start = Instant::now();
    let mut total_confirmed = 0usize;
    let mut height = 1u64;
    let mut prev_hash = blockchain.tip;

    loop {
        let block_txs = mempool.get_transactions_with_limit(BLOCK_BYTE_BUDGET);
        if block_txs.is_empty() { break; }

        // Drain selected transactions from mempool
        for tx in &block_txs {
            if let Some(id) = tx.id {
                mempool.remove_transaction(&id);
            }
        }

        let confirmed = block_txs.len();
        total_confirmed += confirmed;

        let block = mine_block_bypass_pow(prev_hash, block_txs, height);
        prev_hash = block.hash.unwrap();
        blockchain.add_block(block).expect("add_block must not fail");
        height += 1;
    }

    let elapsed = start.elapsed();
    let elapsed_secs = elapsed.as_secs_f64();
    let tps = total_confirmed as f64 / elapsed_secs.max(1e-6);

    // Assertions
    assert_eq!(
        total_confirmed, TX_COUNT,
        "all {TX_COUNT} transactions must be confirmed; got {total_confirmed}"
    );
    assert_eq!(mempool.size(), 0, "mempool must be empty after draining");
    assert!(blockchain.height >= 1, "blockchain must have grown");
    assert!(
        tps >= MIN_TPS,
        "TPS {tps:.1} is below minimum {MIN_TPS} (elapsed={elapsed_secs:.3}s)"
    );

    println!(
        "[TDD-4.1 PASS] TPS={tps:.1} confirmed={total_confirmed} blocks={} elapsed={elapsed_secs:.3}s",
        blockchain.height
    );
}
