// Chain reorganization tests (#820).
//
// Proves that the chain reorg mechanism correctly:
// - Detects fork points between competing chains
// - Plans reorgs when a longer chain is found
// - Rejects reorgs when the local chain is already best
// - Executes reorgs and converges state
// - Respects the MAX_REORG_DEPTH limit
// - Breaks ties deterministically by lowest tip hash
//
// Run from the workspace root:
//   cargo test -p wrfcoin-shared-protocol --test chain_reorg -- --nocapture

use chrono::TimeZone;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::{Blockchain, MAX_REORG_DEPTH};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use std::sync::{Mutex, MutexGuard};

// Every test in this process that can update the process-global reorg metrics
// takes the same lock. Locking only the tests that assert a counter delta is
// insufficient: an otherwise unrelated plan_reorg call can increment between
// the before/after reads and make an exact-delta assertion flaky (#3164).
static METRIC_TEST_LOCK: Mutex<()> = Mutex::new(());

fn metric_test_guard() -> MutexGuard<'static, ()> {
    METRIC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// `Block::new` stamps `Utc::now()`, which makes every block hash differ from run to run. Pin the
/// timestamp per height so fixture hashes are a pure function of (parent, height, nonce) (#3192).
fn pin_timestamp(block: &mut Block, height: u64) {
    block.timestamp = chrono::Utc
        .timestamp_opt(1_700_000_000 + height as i64, 0)
        .single()
        .expect("fixture timestamp should be valid");
}

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

/// Build a chain of `count` blocks starting from the given chain's tip.
/// Each block gets a unique nonce so that block hashes differ between forks.
fn extend_chain(chain: &mut Blockchain, count: u64, nonce_offset: u64) {
    let start = chain.height + 1;
    for i in 0..count {
        let mut block = Block::new(1, chain.tip, vec![], 1, start + i);
        pin_timestamp(&mut block, start + i);
        block.nonce = nonce_offset + i;
        let hash = block.calculate_hash();
        block.set_hash(hash);
        chain.add_block(block).unwrap();
    }
}

/// Build a standalone chain of blocks from the given parent hash and height.
/// Returns the blocks (not added to any chain).
fn build_fork_blocks(
    parent_hash: Hash,
    start_height: u64,
    count: u64,
    nonce_offset: u64,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut prev = parent_hash;
    for i in 0..count {
        let mut block = Block::new(1, prev, vec![], 1, start_height + i);
        pin_timestamp(&mut block, start_height + i);
        block.nonce = nonce_offset + i;
        let hash = block.calculate_hash();
        block.set_hash(hash);
        prev = hash;
        blocks.push(block);
    }
    blocks
}

// -------------------------------------------------------------------
// Test 1: find_fork_point
// -------------------------------------------------------------------

#[test]
fn test_find_fork_point() {
    let mut chain = Blockchain::default();
    // Build a chain of 10 blocks.
    extend_chain(&mut chain, 10, 0);

    // Fork at height 5: candidate blocks start at height 6 with
    // prev_block_hash pointing to the block at height 5.
    let fork_parent = chain.block_at_height(5).unwrap();
    let fork_parent_hash = fork_parent.hash.unwrap_or_else(|| fork_parent.calculate_hash());
    let candidate = build_fork_blocks(fork_parent_hash, 6, 3, 1000);

    let fork_point = chain.find_fork_point(&candidate);
    assert_eq!(fork_point, Some(5), "Fork point should be at height 5");
}

#[test]
fn test_find_fork_point_no_common_ancestor() {
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 5, 0);

    // Build candidate blocks with a random parent hash (no ancestor in chain).
    let fake_parent = Hash::hash_bytes(b"nonexistent");
    let candidate = build_fork_blocks(fake_parent, 3, 3, 999);

    let fork_point = chain.find_fork_point(&candidate);
    assert_eq!(fork_point, None, "Should find no common ancestor");
}

// -------------------------------------------------------------------
// Test 2: plan_reorg — longer chain wins
// -------------------------------------------------------------------

#[test]
fn test_plan_reorg_longer_chain_wins() {
    let _metric_guard = metric_test_guard();
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);

    // Fork at height 8, candidate has 4 more blocks (tip at 12).
    let fork_parent = chain.block_at_height(8).unwrap();
    let fork_parent_hash = fork_parent.hash.unwrap_or_else(|| fork_parent.calculate_hash());
    let candidate = build_fork_blocks(fork_parent_hash, 9, 4, 2000);

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(plan.is_some(), "Should plan a reorg");
    let plan = plan.unwrap();
    assert_eq!(plan.fork_height, 8);
    assert_eq!(plan.blocks_to_revert, 2); // heights 9, 10
    assert_eq!(plan.blocks_to_apply.len(), 4); // heights 9, 10, 11, 12
}

// -------------------------------------------------------------------
// Test 3: plan_reorg — shorter chain rejected
// -------------------------------------------------------------------

#[test]
fn test_plan_reorg_shorter_chain_rejected() {
    let _metric_guard = metric_test_guard();
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);

    // Fork at height 8, candidate has only 1 block (tip at 9).
    let fork_parent = chain.block_at_height(8).unwrap();
    let fork_parent_hash = fork_parent.hash.unwrap_or_else(|| fork_parent.calculate_hash());
    let candidate = build_fork_blocks(fork_parent_hash, 9, 1, 3000);

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(plan.is_none(), "Should reject shorter chain");
}

// -------------------------------------------------------------------
// Test 4: execute_reorg — state converges
// -------------------------------------------------------------------

#[test]
fn test_execute_reorg_state_converges() {
    let _metric_guard = metric_test_guard();
    // Build chain A: 5 common blocks, then 3 blocks on fork A.
    let mut chain_a = Blockchain::default();
    let user_a = addr(1);
    chain_a.state.create_account(user_a, Balance(0));
    extend_chain(&mut chain_a, 5, 0);

    // Save state and tip at fork point for chain B.
    let fork_state = chain_a.state.clone();
    let fork_tip = chain_a.tip;
    let fork_height = chain_a.height;

    // Extend chain A with 3 more blocks.
    extend_chain(&mut chain_a, 3, 100);
    assert_eq!(chain_a.height, 8);

    // Build chain B: same fork point, 5 more blocks (longer).
    let candidate = build_fork_blocks(fork_tip, fork_height + 1, 5, 200);
    assert_eq!(candidate.last().unwrap().height, 10);

    // Plan the reorg.
    let plan = chain_a.plan_reorg(&candidate).unwrap().unwrap();
    assert_eq!(plan.fork_height, 5);
    assert_eq!(plan.blocks_to_revert, 3);
    assert_eq!(plan.blocks_to_apply.len(), 5);

    // Execute the reorg.
    chain_a.execute_reorg(&plan, fork_state).unwrap();

    // Chain A should now be at height 10 with the candidate chain.
    assert_eq!(chain_a.height, 10);
    assert_eq!(
        chain_a.tip,
        candidate.last().unwrap().hash.unwrap()
    );

    // Verify the blocks in memory match the candidate chain.
    for blk in &candidate {
        let in_chain = chain_a.block_at_height(blk.height).unwrap();
        assert_eq!(
            in_chain.hash.unwrap(),
            blk.hash.unwrap(),
            "Block at height {} should match candidate",
            blk.height
        );
    }
}

// -------------------------------------------------------------------
// Test 5: reorg depth limit
// -------------------------------------------------------------------

#[test]
fn test_reorg_depth_limit() {
    let _metric_guard = metric_test_guard();
    let mut chain = Blockchain::default();
    let depth = MAX_REORG_DEPTH + 10;
    extend_chain(&mut chain, depth + 5, 0);

    // Fork at height 5 — candidate forks from height 5 and builds
    // depth + 10 blocks, which means reverting depth + chain.height - 5
    // blocks. With chain at height depth+5 and fork at 5, that's
    // reverting (depth + 5) - 5 = depth blocks, which exceeds the limit.
    let fork_parent = chain.block_at_height(5).unwrap();
    let fork_parent_hash = fork_parent.hash.unwrap_or_else(|| fork_parent.calculate_hash());
    let candidate = build_fork_blocks(
        fork_parent_hash,
        6,
        depth + 20, // longer than local
        5000,
    );

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(
        plan.is_none(),
        "Should reject reorg deeper than MAX_REORG_DEPTH"
    );
}

// -------------------------------------------------------------------
// Test 6: tie-break by lowest tip hash
// -------------------------------------------------------------------

#[test]
fn test_reorg_tiebreak_lowest_hash() {
    let _metric_guard = metric_test_guard();
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 5, 0);

    // Fork at height 3, candidate has 2 blocks (same total height = 5).
    let fork_parent = chain.block_at_height(3).unwrap();
    let fork_parent_hash = fork_parent.hash.unwrap_or_else(|| fork_parent.calculate_hash());

    // Try many nonce offsets to find one where candidate tip < local tip
    // and one where candidate tip > local tip.
    let local_tip = chain.tip;
    let mut found_lower = false;
    let mut found_higher = false;

    for nonce_offset in 0u64..1000 {
        let candidate = build_fork_blocks(fork_parent_hash, 4, 2, nonce_offset * 100);
        let candidate_tip = candidate.last().unwrap().hash.unwrap();

        let plan = chain.plan_reorg(&candidate).unwrap();

        if candidate_tip < local_tip {
            // Lower hash should win — reorg should be planned.
            assert!(
                plan.is_some(),
                "Candidate with lower tip hash should trigger reorg"
            );
            found_lower = true;
        } else {
            // Equal or higher hash — local chain keeps priority.
            assert!(
                plan.is_none(),
                "Candidate with higher/equal tip hash should not trigger reorg"
            );
            found_higher = true;
        }

        if found_lower && found_higher {
            break;
        }
    }

    assert!(
        found_lower && found_higher,
        "Should have found both lower and higher candidate tip hashes"
    );
}

/// #3192: the tie-break test samples candidate tips and needs both a lower and a higher one, which is
/// only a deterministic property when the fixture hashes do not depend on the wall clock.
#[test]
fn fixture_chain_hashes_do_not_depend_on_the_wall_clock_3192() {
    let build = || {
        let mut chain = Blockchain::default();
        extend_chain(&mut chain, 5, 0);
        chain.tip
    };
    let first = build();
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(
        first,
        build(),
        "fixture tip hash changed between builds: wall-clock dependent"
    );
}

// -------------------------------------------------------------------
// Test 7: empty candidate is rejected
// -------------------------------------------------------------------

#[test]
fn test_plan_reorg_empty_candidate() {
    let _metric_guard = metric_test_guard();
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 5, 0);

    let plan = chain.plan_reorg(&[]).unwrap();
    assert!(plan.is_none(), "Empty candidate should be rejected");
}

// -------------------------------------------------------------------
// Test 8: reorg from genesis (fork at height 0)
// -------------------------------------------------------------------

#[test]
fn test_reorg_from_genesis() {
    let _metric_guard = metric_test_guard();
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 3, 0);

    // Build a longer chain from genesis.
    let candidate = build_fork_blocks(Hash::default(), 1, 5, 9000);

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(plan.is_some(), "Should plan reorg from genesis");
    let plan = plan.unwrap();
    assert_eq!(plan.fork_height, 0);
    assert_eq!(plan.blocks_to_revert, 3);
    assert_eq!(plan.blocks_to_apply.len(), 5);
}

// -------------------------------------------------------------------
// Test 9: snapshot-and-restore after execute_reorg (#1025)
//
// Proves that cloning the Blockchain before execute_reorg and restoring
// the snapshot if state transitions fail mid-loop leaves the chain in
// a fully consistent state (height, tip, blocks, state all match).
// -------------------------------------------------------------------

#[test]
fn test_reorg_snapshot_restore_on_failure() {
    let _metric_guard = metric_test_guard();
    // Build a chain of 10 blocks.
    let mut chain = Blockchain::default();
    let user = addr(1);
    chain.state.create_account(user, Balance(0));
    extend_chain(&mut chain, 10, 0);

    // Capture pre-reorg state for later comparison.
    let pre_reorg_height = chain.height;
    let pre_reorg_tip = chain.tip;
    let pre_reorg_state_root = chain.state.state_root;

    // Fork at height 7, candidate has 5 blocks (tip at 12 — longer).
    let fork_parent = chain.block_at_height(7).unwrap();
    let fork_parent_hash = fork_parent.hash.unwrap_or_else(|| fork_parent.calculate_hash());
    let fork_state = chain.state.clone();
    let candidate = build_fork_blocks(fork_parent_hash, 8, 5, 5000);

    // Plan the reorg.
    let plan = chain.plan_reorg(&candidate).unwrap().unwrap();
    assert_eq!(plan.fork_height, 7);
    assert_eq!(plan.blocks_to_revert, 3);
    assert_eq!(plan.blocks_to_apply.len(), 5);

    // Snapshot before execute_reorg — this is the recovery mechanism.
    let snapshot = chain.clone();

    // Execute the reorg — this WILL modify height, tip, and blocks.
    chain.execute_reorg(&plan, fork_state).unwrap();

    // Chain is now at the new fork's tip.
    assert_eq!(chain.height, 12);
    assert_ne!(chain.tip, pre_reorg_tip);

    // Simulate a state transition failure: restore the snapshot.
    chain = snapshot;

    // Verify the chain is back to its pre-reorg state.
    assert_eq!(chain.height, pre_reorg_height);
    assert_eq!(chain.tip, pre_reorg_tip);
    assert_eq!(chain.state.state_root, pre_reorg_state_root);

    // Verify in-memory blocks are correct.
    assert!(
        chain.block_at_height(10).is_some(),
        "Block at original height 10 should still be present"
    );
    assert!(
        chain.block_at_height(11).is_none(),
        "Block at fork height 11 should NOT be present after rollback"
    );

    // Verify the chain can still accept new blocks normally.
    let next_block_height = chain.height + 1;
    let mut next_block = Block::new(1, chain.tip, vec![], 1, next_block_height);
    let hash = next_block.calculate_hash();
    next_block.set_hash(hash);
    chain.add_block(next_block).unwrap();
    assert_eq!(chain.height, 11);
}

// -------------------------------------------------------------------
// Test 10: snapshot preserves total_block_count (#1025)
//
// The Blockchain.total_block_count is not directly accessible except
// via total_block_count(), but it must survive clone-and-restore so
// the chain continuity validation works correctly post-recovery.
// -------------------------------------------------------------------

#[test]
fn test_snapshot_preserves_block_count_and_continuity() {
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 20, 0);
    assert_eq!(chain.total_block_count(), 20);

    let snapshot = chain.clone();
    assert_eq!(snapshot.total_block_count(), 20);
    assert_eq!(snapshot.height, 20);
    assert_eq!(snapshot.tip, chain.tip);

    // After restoring, the chain must accept block at height 21.
    let mut chain = snapshot;
    let mut next = Block::new(1, chain.tip, vec![], 1, 21);
    let h = next.calculate_hash();
    next.set_hash(h);
    chain.add_block(next).unwrap();
    assert_eq!(chain.height, 21);
    assert_eq!(chain.total_block_count(), 21);
}

// -------------------------------------------------------------------
// Chain fork/reorg metric instrumentation (#1148)
// -------------------------------------------------------------------
//
// These tests verify that `plan_reorg` and `execute_reorg` update the
// `CORE_METRICS` counters consumed by `alert-rules-blockchain-health.yml`.
// Because `CORE_METRICS` is a process-wide static, the tests below measure
// DELTAS around each operation (not absolute values) so they remain correct
// when running in parallel with the other reorg tests, and hold a local
// mutex to serialize gauge reads (fork_active, reorg_depth) which are
// racey by design when multiple tests manipulate them concurrently.

#[test]
fn reorg_execution_bumps_reorg_total_and_fork_count() {
    use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

    let _metric_guard = metric_test_guard();

    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 5, 0);
    let fork_tip = chain.tip;
    let fork_state = chain.state.clone();
    extend_chain(&mut chain, 3, 100);

    let candidate = build_fork_blocks(fork_tip, 6, 5, 200);

    let fork_count_before = CORE_METRICS.chain_fork_count.get();
    let reorg_total_before = CORE_METRICS.chain_reorg_total.get();

    let plan = chain.plan_reorg(&candidate).unwrap().unwrap();
    // plan_reorg should have recorded the observation on the "will-reorg" path.
    assert_eq!(
        CORE_METRICS.chain_fork_count.get(),
        fork_count_before + 1,
        "plan_reorg must bump chain_fork_count when returning Some"
    );
    assert_eq!(
        CORE_METRICS.chain_reorg_total.get(),
        reorg_total_before,
        "plan_reorg alone must NOT bump chain_reorg_total"
    );

    chain.execute_reorg(&plan, fork_state).unwrap();
    assert_eq!(
        CORE_METRICS.chain_reorg_total.get(),
        reorg_total_before + 1,
        "execute_reorg must bump chain_reorg_total exactly once"
    );
    assert_eq!(
        CORE_METRICS.chain_reorg_depth.get(),
        3,
        "execute_reorg must pin chain_reorg_depth to the just-reverted depth (3 blocks)"
    );
    assert_eq!(
        CORE_METRICS.chain_fork_active.get(),
        0,
        "successful execute_reorg must clear chain_fork_active"
    );
}

#[test]
fn plan_reorg_rejected_for_max_depth_sets_fork_active() {
    use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

    let _metric_guard = metric_test_guard();

    // Clear any residue from other tests.
    CORE_METRICS.set_fork_active(false);

    let mut chain = Blockchain::default();
    extend_chain(&mut chain, MAX_REORG_DEPTH + 5, 0);

    // Fork at height 2 — 103 blocks deep, beyond MAX_REORG_DEPTH (100).
    let fork_parent_hash = chain.block_at_height(2).unwrap().hash.unwrap();
    let candidate = build_fork_blocks(fork_parent_hash, 3, MAX_REORG_DEPTH + 10, 7000);

    let fork_count_before = CORE_METRICS.chain_fork_count.get();

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(plan.is_none(), "Deeper-than-max reorg must be rejected");
    assert_eq!(
        CORE_METRICS.chain_fork_count.get(),
        fork_count_before + 1,
        "An unresolvable fork must still increment chain_fork_count — operators need to see it"
    );
    assert_eq!(
        CORE_METRICS.chain_fork_active.get(),
        1,
        "A fork that exceeds MAX_REORG_DEPTH must raise chain_fork_active so ChainForkDetected alert fires"
    );

    // Cleanup so the next test starts from a known state.
    CORE_METRICS.set_fork_active(false);
}

#[test]
fn plan_reorg_rejecting_shorter_candidate_does_not_touch_metrics() {
    use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

    let _metric_guard = metric_test_guard();

    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);

    let fork_parent_hash = chain.block_at_height(8).unwrap().hash.unwrap();
    let candidate = build_fork_blocks(fork_parent_hash, 9, 1, 8000); // shorter

    let fork_count_before = CORE_METRICS.chain_fork_count.get();
    let reorg_total_before = CORE_METRICS.chain_reorg_total.get();

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(plan.is_none(), "Shorter candidate must be rejected");

    assert_eq!(
        CORE_METRICS.chain_fork_count.get(),
        fork_count_before,
        "A candidate rejected for being shorter is NOT a real fork observation and must not inflate chain_fork_count"
    );
    assert_eq!(
        CORE_METRICS.chain_reorg_total.get(),
        reorg_total_before,
        "No reorg happened — chain_reorg_total must be unchanged"
    );
}

// -------------------------------------------------------------------
// BFT finality guard (#1216)
// -------------------------------------------------------------------
//
// These tests prove that `plan_reorg` refuses to revert any block at or
// below the BFT-finalized height, regardless of candidate chain length.

#[test]
fn plan_reorg_rejects_fork_below_finalized_height() {
    let _metric_guard = metric_test_guard();
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);

    // Mark height 7 as BFT-finalized.
    chain.set_finalized_height(7);
    assert_eq!(chain.finalized_height(), 7);

    // Candidate forks at height 5 — below the finalized height.
    // Even a much longer candidate must be refused.
    let fork_parent = chain.block_at_height(5).unwrap();
    let fork_parent_hash = fork_parent.hash.unwrap_or_else(|| fork_parent.calculate_hash());
    let candidate = build_fork_blocks(fork_parent_hash, 6, 20, 7000); // tip at 25

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(
        plan.is_none(),
        "Reorg that reverts finalized blocks must be refused"
    );
}

#[test]
fn plan_reorg_rejects_fork_at_finalized_height() {
    let _metric_guard = metric_test_guard();
    // Fork point == finalized height: the block at finalized height
    // would be KEPT, but the test guard uses strict `<` so this is
    // actually allowed. We test the boundary to make the semantics
    // explicit: only forks STRICTLY BELOW finalized_height are refused.
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);
    chain.set_finalized_height(5);

    let fork_parent = chain.block_at_height(5).unwrap();
    let fork_parent_hash = fork_parent.hash.unwrap_or_else(|| fork_parent.calculate_hash());
    let candidate = build_fork_blocks(fork_parent_hash, 6, 8, 7100); // tip at 13

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(
        plan.is_some(),
        "A reorg forking AT the finalized height is allowed — \
         it keeps the finalized block and replaces only blocks above it"
    );
    assert_eq!(plan.unwrap().fork_height, 5);
}

#[test]
fn plan_reorg_allows_fork_above_finalized_height() {
    let _metric_guard = metric_test_guard();
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 20, 0);
    chain.set_finalized_height(10);

    // Fork at height 15, above the finalized height — should be allowed.
    let fork_parent = chain.block_at_height(15).unwrap();
    let fork_parent_hash = fork_parent.hash.unwrap_or_else(|| fork_parent.calculate_hash());
    let candidate = build_fork_blocks(fork_parent_hash, 16, 8, 7200); // tip at 23

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(plan.is_some(), "Fork above finalized height must be allowed");
    assert_eq!(plan.unwrap().fork_height, 15);
}

#[test]
fn set_finalized_height_is_monotonic() {
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);

    chain.set_finalized_height(5);
    assert_eq!(chain.finalized_height(), 5);

    // Increasing is allowed.
    chain.set_finalized_height(8);
    assert_eq!(chain.finalized_height(), 8);

    // Decreasing is silently ignored (prevents accidental rollback).
    chain.set_finalized_height(3);
    assert_eq!(chain.finalized_height(), 8, "finalized height must not roll back");

    // Same value is a no-op.
    chain.set_finalized_height(8);
    assert_eq!(chain.finalized_height(), 8);
}

#[test]
fn default_finalized_height_is_zero() {
    let chain = Blockchain::default();
    assert_eq!(
        chain.finalized_height(),
        0,
        "Default Blockchain must have finalized_height=0 so BFT-unaware \
         callers see unchanged behavior"
    );
}

#[test]
fn plan_reorg_finalized_guard_records_fork_metrics() {
    use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

    let _metric_guard = metric_test_guard();

    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);
    chain.set_finalized_height(7);

    let fork_parent_hash = chain.block_at_height(4).unwrap().hash.unwrap();
    let candidate = build_fork_blocks(fork_parent_hash, 5, 10, 7300); // longer

    let fork_count_before = CORE_METRICS.chain_fork_count.get();

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(plan.is_none(), "Must refuse reorg below finalized height");

    // The rejected-by-finality path still bumps fork metrics so operators
    // can observe that a conflicting chain tried to revert finalized state.
    assert!(
        CORE_METRICS.chain_fork_count.get() > fork_count_before,
        "BFT-finality rejection must still count as an observed fork"
    );

    // Clean up so other tests start from a known metric state.
    CORE_METRICS.set_fork_active(false);
}
