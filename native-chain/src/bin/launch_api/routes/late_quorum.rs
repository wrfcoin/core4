//! Durable reconciliation for the #2464 late-quorum unpaid count.
//!
//! The detector (#2464 phase 1, `proof_of_publishing.rs`) already writes
//! [`LATE_QUORUM_COUNT_METADATA_KEY`] and [`LATE_QUORUM_COMMIT_IDS_METADATA_KEY`]
//! into every block it produces, so the record of "this contribution reached
//! oracle quorum after its reward decision had already closed" is durable the
//! moment the block is persisted.
//!
//! What was NOT durable is the *reading* of it. The original #2464 total summed
//! `Blockchain::blocks()`, which is a bounded ring of
//! `configured_max_in_memory_blocks()` entries (default 200) that evicts the
//! oldest block on every apply. So a block that recorded unpaid contributions
//! stopped contributing to the total ~200 blocks later, and the number decayed
//! toward zero on a healthy-looking chain. A reconciliation figure that decays
//! to zero reads as "the backlog was cleared" when in fact nothing was paid —
//! precisely the failure mode that let 5,578 observations produce zero WRF
//! without leaving a countable trace.
//!
//! This module reads the **durable block archive** instead, so the count is a
//! property of the chain rather than of one node's cache window, and exposes it
//! per-block with commit IDs so the backlog can be enumerated and reconciled
//! rather than merely totalled.
//!
//! This is the reconciliation surface only. Paying the backlog is #2464 phase 2
//! and settles through the shared apply sink in #2508; nothing here mutates
//! state, emits a transaction, or participates in `state_root`.

// Every read here surfaces `StorageError`, which is a large enum. Boxing it at
// this one call site would diverge from `storage.rs` and `evidence_store.rs`,
// which take the same allow for the same reason.
#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use actix_web::{HttpRequest, Responder, http::StatusCode, web};
use serde::Deserialize;
use serde_json::json;

use crate::helpers::{envelope_error, envelope_success, require_role};
use crate::state::AppState;
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::consensus::proof_of_publishing::{
    LATE_QUORUM_COMMIT_IDS_METADATA_KEY, LATE_QUORUM_COUNT_METADATA_KEY,
};
use wrfcoin_native_chain::storage::{Storage, StorageError};
use wrfcoin_shared_protocol::blockchain::chain::MAX_REORG_DEPTH;
use wrfcoin_shared_protocol::security::api_auth::ApiRole;

/// Default number of per-block entries returned by the reconciliation endpoint.
const DEFAULT_ENTRY_LIMIT: usize = 500;

/// Upper bound on `limit`, so a single call cannot be made to materialise an
/// unbounded response out of a long archive.
const MAX_ENTRY_LIMIT: usize = 5_000;

/// One block's late-quorum detector reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateQuorumEntry {
    pub height: u64,
    pub count: u64,
    pub commit_ids: Vec<String>,
}

/// Read the detector keys off a single block.
///
/// Returns `None` for the two indistinguishable-at-a-glance but very different
/// "nothing to see here" cases, both of which must contribute 0 rather than
/// poison the total:
///
/// - the key is absent — the block predates the detector;
/// - the key is present and 0 — the detector ran and found nothing.
///
/// A malformed count is treated as absent rather than as an error: this is a
/// reporting path, and one corrupt block must not make the whole backlog
/// unreadable.
fn entry_from_block(block: &Block) -> Option<LateQuorumEntry> {
    let count = block
        .metadata
        .get(LATE_QUORUM_COUNT_METADATA_KEY)
        .and_then(|raw| raw.parse::<u64>().ok())?;
    if count == 0 {
        return None;
    }
    let commit_ids = block
        .metadata
        .get(LATE_QUORUM_COMMIT_IDS_METADATA_KEY)
        .map(|csv| {
            csv.split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some(LateQuorumEntry {
        height: block.height,
        count,
        commit_ids,
    })
}

/// Fold the detector counts for every block in the durable archive into
/// `sink`, oldest height first, stopping once `stop_after` is passed.
///
/// Uses the chunked whole-archive scan so memory stays bounded by
/// [`wrfcoin_native_chain::storage::PERSISTED_BLOCK_SCAN_CHUNK`] regardless of
/// chain length, and so the whole fold observes one consistent redb snapshot.
fn fold_archive<F>(storage: &Storage, stop_after: Option<u64>, mut sink: F) -> Result<(), StorageError>
where
    F: FnMut(LateQuorumEntry),
{
    storage.scan_persisted_blocks(|chunk| {
        for (_key, block) in chunk {
            if let Some(stop_after) = stop_after {
                if block.height > stop_after {
                    return std::ops::ControlFlow::Break(());
                }
            }
            if let Some(entry) = entry_from_block(block) {
                sink(entry);
            }
        }
        std::ops::ControlFlow::Continue(())
    })?;
    Ok(())
}

/// Fold a bounded height range out of the durable archive.
fn fold_archive_range<F>(
    storage: &Storage,
    start: u64,
    end: u64,
    mut sink: F,
) -> Result<(), StorageError>
where
    F: FnMut(LateQuorumEntry),
{
    if start > end {
        return Ok(());
    }
    for block in storage.load_blocks_range(start, end)? {
        if let Some(entry) = entry_from_block(&block) {
            sink(entry);
        }
    }
    Ok(())
}

/// Memo for the *final* portion of the archive — heights at or below
/// `tip - MAX_REORG_DEPTH`, which by definition can no longer be rewritten.
///
/// Folding only final heights is what makes the running total safe to
/// accumulate rather than recompute: a height is added exactly once and can
/// never be reorged out from under the sum afterwards. The recent tail above
/// the finality line is deliberately NOT memoized — it is recomputed on every
/// call, so replay, reorg re-apply and WAL restore cannot double-count, which
/// is the same property the original per-scrape recomputation had.
#[derive(Debug, Clone, Copy, Default)]
struct FinalizedBase {
    /// Highest height folded into `total`. 0 means "nothing folded yet".
    folded_through: u64,
    total: u64,
}

/// Run `f` against the finalized-base memo.
///
/// **Production**: one process-global memo, as before — a node has exactly one
/// chain, so a single accumulator is correct and is what makes a scrape cheap.
///
/// **Tests**: the memo is *thread-local* instead. A test binary runs many chains
/// in one process, and the memo is keyed only by height: two different chains
/// with the same `folded_through` silently share a fold. `reset_finalized_base`
/// guards that within this module, but `unpaid_total` is also reachable from the
/// public `/metrics` route (`routes::chain::get_prometheus_metrics`), which ten
/// tests in `main.rs` exercise. Those run concurrently with these, fold a
/// *different* chain into the shared memo between a reset and its read, and
/// produce a wrong answer — a real flake, measured at ~1 run in 10 of
/// `--bin launch_api`.
///
/// Serializing the eleven call sites would work only until the twelfth is
/// written, and would require every future `/metrics` test to know about a memo
/// three modules away. Thread-locality makes the isolation structural: cargo
/// parallelizes over test threads, and `#[actix_web::test]` pins a test to one
/// thread, so no test can observe another's fold.
///
/// Residual, deliberately not addressed here: two chains used *sequentially on
/// one thread* still share the memo. That is what `reset_finalized_base` is for,
/// it is local to this file, and every test here already calls it. The root
/// cause — a memo keyed by height rather than by chain — is a separate change.
fn with_finalized_base<R>(f: impl FnOnce(&mut FinalizedBase) -> R) -> R {
    #[cfg(not(test))]
    {
        static BASE: OnceLock<Mutex<FinalizedBase>> = OnceLock::new();
        let cell = BASE.get_or_init(|| Mutex::new(FinalizedBase::default()));
        // panic-ok: mutex poisoning only, never decoded input — reachable solely after another thread panicked holding this lock; same shape as the baselined cache mutexes in chain.rs, mining.rs and stake.rs
        let mut guard = cell.lock().expect("late-quorum base poisoned");
        f(&mut guard)
    }
    #[cfg(test)]
    {
        thread_local! {
            static BASE: std::cell::RefCell<FinalizedBase> =
                const { std::cell::RefCell::new(FinalizedBase { folded_through: 0, total: 0 }) };
        }
        BASE.with(|cell| f(&mut cell.borrow_mut()))
    }
}

/// Reset the memo. Test-only: a test that builds a fresh chain must not inherit
/// a previous test's fold on the same thread.
#[cfg(test)]
fn reset_finalized_base() {
    with_finalized_base(|base| *base = FinalizedBase::default());
}

/// Total unpaid contributions recorded across the **whole durable archive**.
///
/// Computed as `finalized base + recent tail`:
///
/// - the base covers heights `..= tip - MAX_REORG_DEPTH` and is accumulated
///   incrementally, so the steady-state cost of a Prometheus scrape is a
///   ranged read of the one or two heights that just became final, not a scan
///   of the chain;
/// - the tail covers the last [`MAX_REORG_DEPTH`] heights and is recomputed
///   every call from the archive, overlaid with the in-memory window so a block
///   that has been applied but not yet persisted still counts.
///
/// If the tip has moved *below* what was already folded — a rollback, or simply
/// a different chain sharing this process in tests — the memo is discarded and
/// rebuilt from scratch. That path is slower but always correct; the memo can
/// degrade throughput, never the answer.
pub fn unpaid_total(blockchain: &Blockchain, storage: &Storage) -> Result<u64, StorageError> {
    let tip_height = blockchain.height;
    let final_through = tip_height.saturating_sub(MAX_REORG_DEPTH);

    let base_total = with_finalized_base(|base| -> Result<u64, StorageError> {
        if base.folded_through > final_through {
            *base = FinalizedBase::default();
        }
        if base.folded_through < final_through {
            let mut added = 0u64;
            if base.folded_through == 0 {
                fold_archive(storage, Some(final_through), |entry| {
                    added = added.saturating_add(entry.count);
                })?;
            } else {
                fold_archive_range(storage, base.folded_through + 1, final_through, |entry| {
                    added = added.saturating_add(entry.count);
                })?;
            }
            base.total = base.total.saturating_add(added);
            base.folded_through = final_through;
        }
        Ok(base.total)
    })?;

    // Recent tail: archive first, then overlay the in-memory window so a block
    // applied but not yet persisted is not missed. Keyed by height so a block
    // present in both is counted once.
    let mut tail: BTreeMap<u64, u64> = BTreeMap::new();
    fold_archive_range(storage, final_through + 1, tip_height, |entry| {
        tail.insert(entry.height, entry.count);
    })?;
    for block in blockchain.blocks_range(final_through + 1, tip_height) {
        if let Some(entry) = entry_from_block(block) {
            tail.insert(entry.height, entry.count);
        }
    }
    let tail_total = tail.values().fold(0u64, |acc, c| acc.saturating_add(*c));

    Ok(base_total.saturating_add(tail_total))
}

/// Enumerate the unpaid backlog per block across the whole durable archive.
///
/// This is the reconciliation read: it answers "which commits", not just "how
/// many". `total_unpaid` always describes the entire archive regardless of
/// paging, so an operator paging through entries never sees the headline figure
/// change under them.
pub fn unpaid_entries(
    blockchain: &Blockchain,
    storage: &Storage,
    from_height: u64,
    limit: usize,
) -> Result<(u64, u64, Vec<LateQuorumEntry>, bool), StorageError> {
    let mut all: BTreeMap<u64, LateQuorumEntry> = BTreeMap::new();
    fold_archive(storage, None, |entry| {
        all.insert(entry.height, entry);
    })?;
    // Overlay the in-memory window: blocks applied but not yet persisted.
    for block in blockchain.blocks().values() {
        if let Some(entry) = entry_from_block(block) {
            all.insert(entry.height, entry);
        }
    }

    let total_unpaid = all
        .values()
        .fold(0u64, |acc, e| acc.saturating_add(e.count));
    let blocks_with_unpaid = all.len() as u64;

    let mut page: Vec<LateQuorumEntry> = all
        .range(from_height..)
        .map(|(_, entry)| entry.clone())
        .collect();
    let truncated = page.len() > limit;
    page.truncate(limit);

    Ok((total_unpaid, blocks_with_unpaid, page, truncated))
}

#[derive(Debug, Deserialize)]
pub struct UnpaidQuery {
    /// Lowest block height to return entries from, for paging. Default 0.
    from_height: Option<u64>,
    /// Maximum number of per-block entries to return.
    limit: Option<usize>,
}

/// `GET /oracle/late-quorum/unpaid` — durable reconciliation of the #2464 gap.
pub async fn get_late_quorum_unpaid(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<UnpaidQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }

    let from_height = query.from_height.unwrap_or(0);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_ENTRY_LIMIT)
        .clamp(1, MAX_ENTRY_LIMIT);

    let blockchain = state.blockchain.read().await;
    let storage = state.storage.read().await;

    match unpaid_entries(&blockchain, &storage, from_height, limit) {
        Ok((total_unpaid, blocks_with_unpaid, entries, truncated)) => {
            let entries: Vec<_> = entries
                .into_iter()
                .map(|e| {
                    json!({
                        "height": e.height,
                        "count": e.count,
                        "commit_ids": e.commit_ids,
                    })
                })
                .collect();
            envelope_success(
                &req,
                StatusCode::OK,
                json!({
                    "total_unpaid": total_unpaid,
                    "blocks_with_unpaid": blocks_with_unpaid,
                    "chain_height": blockchain.height,
                    "from_height": from_height,
                    "limit": limit,
                    "truncated": truncated,
                    "entries": entries,
                }),
            )
        },
        Err(e) => {
            log::error!("late-quorum reconciliation read failed: {e}");
            envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_READ_FAILED",
                "could not read the durable block archive",
            )
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_native_chain::storage::Storage;

    /// Set beyond any chain these tests build, so no block is ever checkpoint-due.
    /// Checkpointing is orthogonal to what is under test here, and a due
    /// checkpoint would require state bytes these fixtures do not produce.
    const TEST_CHECKPOINT_INTERVAL: u64 = 100_000;

    /// Build a chain whose blocks carry the given detector counts, persisting
    /// every block to the durable archive.
    ///
    /// `window` is the in-memory retention size, so a test can force eviction
    /// without producing thousands of blocks.
    async fn chain_with_counts(
        window: usize,
        counts: &[(u64, Option<u64>)],
    ) -> (Blockchain, Storage) {
        let tmp = std::env::temp_dir()
            .join(format!("wrfcoin-late-quorum-{}.redb", fastrand::u64(..)));
        let mut storage =
            Storage::open(&tmp, TEST_CHECKPOINT_INTERVAL).expect("test storage should open");
        let mut blockchain = Blockchain::with_capacity(window);
        let mut parent = blockchain.tip;

        for (height, count) in counts {
            let mut block = Block::new(1, parent, vec![], 1, *height);
            if let Some(count) = count {
                block.metadata.insert(
                    LATE_QUORUM_COUNT_METADATA_KEY.to_string(),
                    count.to_string(),
                );
                let ids: Vec<String> =
                    (0..*count).map(|i| format!("commit-{height}-{i}")).collect();
                block.metadata.insert(
                    LATE_QUORUM_COMMIT_IDS_METADATA_KEY.to_string(),
                    ids.join(","),
                );
            }
            let hash = block.calculate_hash();
            block.set_hash(hash);
            parent = hash;
            storage
                .persist_block(block.clone(), vec![], None)
                .await
                .expect("persist block");
            blockchain.add_block(block).expect("add block");
        }
        (blockchain, storage)
    }

    /// The defect this module exists to fix: a block that recorded unpaid
    /// contributions is evicted from the in-memory ring after
    /// `max_in_memory_blocks` applies, and the window-scoped total silently
    /// dropped its count. The durable read must still see it.
    #[actix_web::test]
    #[serial_test::serial(late_quorum_base)]
    async fn unpaid_total_survives_eviction_from_the_in_memory_window() {
        reset_finalized_base();
        // Window of 4: height 1 records 3 unpaid, then 5 more blocks push it out.
        let mut counts = vec![(1u64, Some(3u64))];
        for h in 2..=6u64 {
            counts.push((h, None));
        }
        let (blockchain, storage) = chain_with_counts(4, &counts).await;

        assert!(
            !blockchain.blocks().contains_key(&1),
            "precondition: height 1 must have been evicted from the window"
        );

        assert_eq!(
            unpaid_total(&blockchain, &storage).expect("durable total"),
            3,
            "the count must be read from the durable archive, not the window; \
             an evicted block still recorded unpaid contributions"
        );
    }

    /// Absent keys (pre-detector blocks) contribute 0, present-and-zero
    /// contributes 0, and only real counts sum.
    #[actix_web::test]
    #[serial_test::serial(late_quorum_base)]
    async fn unpaid_total_sums_only_real_counts() {
        reset_finalized_base();
        let (blockchain, storage) =
            chain_with_counts(200, &[(1, None), (2, Some(2)), (3, Some(0)), (4, Some(5))]).await;
        assert_eq!(
            unpaid_total(&blockchain, &storage).expect("durable total"),
            7
        );
    }

    /// A clean chain must read exactly 0 — "healthy" has to be distinguishable
    /// from "never measured".
    #[actix_web::test]
    #[serial_test::serial(late_quorum_base)]
    async fn unpaid_total_is_zero_on_a_clean_chain() {
        reset_finalized_base();
        let (blockchain, storage) = chain_with_counts(200, &[(1, None), (2, Some(0))]).await;
        assert_eq!(
            unpaid_total(&blockchain, &storage).expect("durable total"),
            0
        );
    }

    /// Exercise the incremental finalized-base memo: with a tip above
    /// `MAX_REORG_DEPTH`, early heights fall below the finality line and are
    /// folded into the accumulated base rather than recomputed from the tail.
    /// The total must be identical to the un-memoized answer, and repeated
    /// calls must not double-count the folded heights.
    #[actix_web::test]
    #[serial_test::serial(late_quorum_base)]
    async fn finalized_base_folds_once_and_matches_the_tail_only_answer() {
        reset_finalized_base();
        // Height 1 (final: below tip - 100) and height 115 (in the tail).
        let mut counts: Vec<(u64, Option<u64>)> = vec![(1, Some(4))];
        for h in 2..=120u64 {
            counts.push((h, if h == 115 { Some(6) } else { None }));
        }
        let (blockchain, storage) = chain_with_counts(200, &counts).await;
        assert!(
            blockchain.height > MAX_REORG_DEPTH,
            "precondition: the chain must extend past the finality line"
        );

        let first = unpaid_total(&blockchain, &storage).expect("first read");
        let second = unpaid_total(&blockchain, &storage).expect("second read");

        assert_eq!(first, 10, "4 below the finality line + 6 in the tail");
        assert_eq!(
            second, first,
            "the memo must be idempotent: a second scrape must not re-add the \
             already-folded finalized heights"
        );
    }

    /// Reconciliation, not just counting: the backlog must be enumerable down
    /// to the individual commit IDs.
    #[actix_web::test]
    #[serial_test::serial(late_quorum_base)]
    async fn unpaid_entries_enumerate_commit_ids_across_the_archive() {
        reset_finalized_base();
        let (blockchain, storage) =
            chain_with_counts(200, &[(1, Some(2)), (2, None), (3, Some(1))]).await;

        let (total, blocks, entries, truncated) =
            unpaid_entries(&blockchain, &storage, 0, 100).expect("entries");

        assert_eq!(total, 3);
        assert_eq!(blocks, 2);
        assert!(!truncated);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].height, 1);
        assert_eq!(
            entries[0].commit_ids,
            vec!["commit-1-0".to_string(), "commit-1-1".to_string()],
            "the commit IDs are what make the backlog reconcilable"
        );
        assert_eq!(entries[1].height, 3);
        assert_eq!(entries[1].commit_ids, vec!["commit-3-0".to_string()]);
    }

    /// Paging must not change the headline figure: an operator walking the
    /// backlog page by page must see one stable total.
    #[actix_web::test]
    #[serial_test::serial(late_quorum_base)]
    async fn total_is_stable_across_paging() {
        reset_finalized_base();
        let (blockchain, storage) =
            chain_with_counts(200, &[(1, Some(1)), (2, Some(1)), (3, Some(1))]).await;

        let (total_first, _, page, truncated) =
            unpaid_entries(&blockchain, &storage, 0, 1).expect("page 1");
        assert_eq!(total_first, 3);
        assert!(truncated);
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].height, 1);

        let (total_second, _, page, _) =
            unpaid_entries(&blockchain, &storage, 3, 1).expect("page 2");
        assert_eq!(total_second, 3, "total describes the archive, not the page");
        assert_eq!(page[0].height, 3);
    }
}
