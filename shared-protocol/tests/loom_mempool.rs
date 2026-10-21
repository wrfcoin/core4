//! Loom interleaving models for the mempool hot path (#2102 story #2109).
//!
//! Rust prevents data races, not logic races. Loom explores thread interleavings
//! exhaustively within a bounded model, where a stress test only hits them by luck.
//!
//! # These drive the production `Mempool`, not a stand-in
//!
//! Every model below puts a real `wrfcoin_shared_protocol::mempool::pool::Mempool`
//! inside a `loom::sync::Mutex` and calls its real methods — `add_transaction`,
//! `remove_transaction`, `size`, `get_transaction`. The assertions are on real
//! mempool state, so a regression in the production type shows up here.
//!
//! # Why there is no `--cfg loom`, and why that is not a shortcut
//!
//! `--cfg loom` exists to swap `std::sync` for `loom::sync` *inside instrumented
//! code*. `Mempool` holds no synchronization primitives at all — it is a plain
//! struct of `FxHashMap`/`BTreeMap` fields, and every caller wraps it externally
//! (`Arc<RwLock<Mempool>>` at `consensus-engines/src/engine.rs:297`,
//! `native-chain/src/bin/launch_api/p2p_events.rs:280`). So the only
//! synchronization in these models is the `loom::sync::Mutex` the model itself
//! creates, which loom already controls. There is nothing left to instrument.
//!
//! That is what makes these models runnable at all. `--cfg loom` is a global flag:
//! it strips `tokio::net` (which is `cfg(not(loom))`), which breaks `hyper-util`,
//! which shared-protocol pulls transitively through `reqwest`. Under `--cfg loom`
//! this crate cannot compile, so no model could ever execute. Reviewers can
//! confirm that in one command:
//!
//! ```text
//! RUSTFLAGS="--cfg loom" cargo test -p wrfcoin-shared-protocol --test loom_mempool
//! error[E0432]: unresolved import `tokio::net`
//!   --> hyper-util-0.1.20/src/client/legacy/connect/http.rs:16:12
//! ```
//!
//! # What these models cover, and what they do not
//!
//! **Covered:** the *lock discipline* callers use around a shared `Mempool` —
//! whether a sequence of real mempool operations, interleaved every way loom can
//! schedule them, preserves the mempool's own invariants.
//!
//! **Not covered:** the production locks themselves. Callers hold
//! `Arc<tokio::sync::RwLock<Mempool>>` — an *async* lock. Loom models threads and
//! std-style synchronization; it does not explore tokio's task scheduler, so the
//! async acquisition order at those call sites is out of scope here. A model of a
//! `loom::sync::Mutex` is a model of mutual exclusion, which is the property those
//! call sites rely on, but it is not a model of tokio.
//!
//! Run: `cargo test -p wrfcoin-shared-protocol --test loom_mempool`
//! Bound: 2 threads, 2 transactions, `LOOM_MAX_PREEMPTIONS=3` (workflow default).

use loom::sync::{Arc as LoomArc, Mutex};
use loom::thread;
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput,
};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::mempool::pool::Mempool;

/// A structurally valid transaction with a deterministic, tag-derived id.
fn tx(tag: &str) -> Transaction {
    let mut t = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("loom-input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: format!("wrf-loom-{tag}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    t.set_id();
    t
}

fn new_pool() -> Mempool {
    Mempool::new(&Config::default())
}

/// Two threads each admit a distinct transaction: neither is lost, neither is
/// duplicated, and `size()` agrees with the number actually resident.
///
/// This is the real `add_transaction` path, so the fee-order index, the
/// sort-key cache and size accounting are all updated by production code under
/// every interleaving loom can produce.
#[test]
fn concurrent_add_transaction_no_loss_no_duplicate() {
    loom::model(|| {
        let pool = LoomArc::new(Mutex::new(new_pool()));
        let (a_tx, b_tx) = (tx("a"), tx("b"));
        let (a_id, b_id) = (a_tx.id, b_tx.id);

        let a = {
            let pool = pool.clone();
            thread::spawn(move || {
                let _ = pool.lock().unwrap().add_transaction(a_tx);
            })
        };
        let b = {
            let pool = pool.clone();
            thread::spawn(move || {
                let _ = pool.lock().unwrap().add_transaction(b_tx);
            })
        };
        a.join().unwrap();
        b.join().unwrap();

        let guard = pool.lock().unwrap();
        // Both admitted exactly once, whichever order they interleaved in.
        let resident = [a_id, b_id]
            .iter()
            .filter(|id| id.map(|h| guard.get_transaction(&h).is_some()).unwrap_or(false))
            .count();
        assert_eq!(
            resident, 2,
            "an interleaving lost a transaction: {resident} of 2 resident"
        );
        // Size accounting must agree with residency — a stale counter is the
        // failure mode a lost update produces.
        assert_eq!(
            guard.size(),
            2,
            "size() disagrees with residency after concurrent admission"
        );
    });
}

/// Concurrent admit + remove of the *same* transaction leaves the mempool
/// self-consistent: `size()` matches `get_transaction`, never a phantom entry.
///
/// The interesting interleaving is remove-before-add, where the remove finds
/// nothing and the add then succeeds. Both orders must leave consistent state.
#[test]
fn concurrent_add_and_remove_keeps_size_consistent() {
    loom::model(|| {
        let pool = LoomArc::new(Mutex::new(new_pool()));
        let victim = tx("victim");
        let victim_id = victim.id.expect("set_id populates the id");

        let adder = {
            let pool = pool.clone();
            thread::spawn(move || {
                let _ = pool.lock().unwrap().add_transaction(victim);
            })
        };
        let remover = {
            let pool = pool.clone();
            thread::spawn(move || {
                pool.lock().unwrap().remove_transaction(&victim_id)
            })
        };
        adder.join().unwrap();
        let removed = remover.join().unwrap();

        let guard = pool.lock().unwrap();
        let present = guard.get_transaction(&victim_id).is_some();
        // Exactly one of: the remove won and nothing is resident, or the add won
        // and it is. Never "removed AND still present", never "not removed AND absent".
        assert_eq!(
            guard.size(),
            usize::from(present),
            "size() {} disagrees with residency {present} (removed={removed})",
            guard.size()
        );
    });
}

/// **The check-then-act model.** Two threads each perform the read-decide-write
/// sequence callers actually use: look for a transaction, and admit it only if
/// absent — but across *two separate* lock acquisitions.
///
/// This is the real hazard at a `Arc<RwLock<Mempool>>` call site: the lock is
/// released between the check and the act, so both threads can observe "absent"
/// and both proceed. The invariant is that the mempool tolerates it — a
/// duplicate admission must not corrupt size accounting.
///
/// This model is also the one the acceptance probe breaks: see the PR body.
#[test]
fn check_then_act_admission_cannot_corrupt_accounting() {
    loom::model(|| {
        let pool = LoomArc::new(Mutex::new(new_pool()));
        let contested = tx("contested");
        let contested_id = contested.id.expect("set_id populates the id");

        let spawn_admitter = |pool: LoomArc<Mutex<Mempool>>, t: Transaction| {
            thread::spawn(move || {
                // Acquire #1: check.
                let absent = pool.lock().unwrap().get_transaction(&contested_id).is_none();
                // Lock released here — this is the gap the real call sites have.
                if absent {
                    // Acquire #2: act.
                    let _ = pool.lock().unwrap().add_transaction(t);
                }
            })
        };

        let a = spawn_admitter(pool.clone(), contested.clone());
        let b = spawn_admitter(pool.clone(), contested);
        a.join().unwrap();
        b.join().unwrap();

        let guard = pool.lock().unwrap();
        // Both threads may have decided to admit. The mempool must still hold the
        // transaction at most once and report a size that matches.
        let present = guard.get_transaction(&contested_id).is_some();
        assert!(present, "check-then-act dropped the transaction entirely");
        assert_eq!(
            guard.size(),
            1,
            "duplicate admission through the check-then-act gap corrupted size() to {}",
            guard.size()
        );
    });
}
