// Reorg convergence integration tests (#967).
//
// Builds on chain_reorg.rs (#820) by testing convergence *properties*:
// - State after reorg matches a fresh chain that never saw the orphaned fork.
// - Balances and accounts revert cleanly when the orphaned fork contained
//   state transitions (transfers, rewards).
// - Deep reorgs near MAX_REORG_DEPTH behave correctly.
// - Out-of-order block arrival still produces a correct chain.
// - Commit-reveal state is correctly expired / reverted across reorgs.
//
// Run from the workspace root:
//   cargo test -p wrfcoin-shared-protocol --test reorg_convergence -- --nocapture

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::{Blockchain, MAX_REORG_DEPTH};
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{CommitPayload, RevealPayload, Transaction};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::commit_window_blocks;

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

/// Extend a chain by `count` blocks with unique nonces.
fn extend_chain(chain: &mut Blockchain, count: u64, nonce_offset: u64) {
    let start = chain.height + 1;
    for i in 0..count {
        let height = start + i;
        let mut block = Block::new(1, chain.tip, vec![], 1, height);
        block.timestamp = chrono::DateTime::from_timestamp(
            i64::try_from(height).expect("test block height fits i64"),
            0,
        )
        .expect("test block height is a valid deterministic timestamp");
        block.nonce = nonce_offset + i;
        let hash = block.calculate_hash();
        block.set_hash(hash);
        chain.add_block(block).unwrap();
    }
}

/// Build detached fork blocks from a given parent hash and starting height.
fn build_fork_blocks(
    parent_hash: wrfcoin_shared_protocol::crypto::hash::Hash,
    start_height: u64,
    count: u64,
    nonce_offset: u64,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut prev = parent_hash;
    for i in 0..count {
        let height = start_height + i;
        let mut block = Block::new(1, prev, vec![], 1, height);
        block.timestamp = chrono::DateTime::from_timestamp(
            i64::try_from(height).expect("test block height fits i64"),
            0,
        )
        .expect("test block height is a valid deterministic timestamp");
        block.nonce = nonce_offset + i;
        let hash = block.calculate_hash();
        block.set_hash(hash);
        prev = hash;
        blocks.push(block);
    }
    blocks
}

#[test]
fn independently_built_fixture_prefixes_are_bit_identical_2490() {
    let first = build_fork_blocks(Default::default(), 1, 5, 0);
    let second = build_fork_blocks(Default::default(), 1, 5, 0);

    for (left, right) in first.iter().zip(second.iter()) {
        assert_eq!(
            left.timestamp, right.timestamp,
            "fixture timestamps must not depend on the wall clock"
        );
        assert_eq!(
            left.hash, right.hash,
            "identical fixture inputs must produce identical block hashes"
        );
    }
}

/// Build a commit/reveal payload pair for a given station, tag, and timestamp.
fn build_commit_reveal(
    station_id: &str,
    tag: &str,
    timestamp: i64,
) -> (CommitPayload, RevealPayload) {
    let payload_data =
        serde_json::json!({"quality": 0.9, "volume": 1.0, "uniqueness": 1.0});
    let nonce = format!("nonce-{tag}");
    let commit_id = Transaction::compute_commit_id(
        &payload_data,
        &nonce,
        station_id,
        timestamp,
        "contribution",
    );

    let commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };

    let reveal = RevealPayload {
        commit_id,
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        data: payload_data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: None,
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };

    (commit, reveal)
}

// -------------------------------------------------------------------
// Test 1: State convergence — reorg reverts balance transfers
// -------------------------------------------------------------------

#[test]
fn reorg_reverts_balance_transfers() {
    let user_a = addr(1);
    let user_b = addr(2);

    // Build a chain with two funded accounts.
    let mut chain = Blockchain::default();
    chain.state.create_account(user_a, Balance(1_000_000));
    chain.state.create_account(user_b, Balance(500_000));

    // Build 5 common blocks.
    extend_chain(&mut chain, 5, 0);

    // Snapshot state at fork point.
    let fork_state = chain.state.clone();
    let fork_tip = chain.tip;
    let fork_height = chain.height;

    // On the local fork (heights 6..8), transfer 100_000 from A to B.
    chain
        .apply_state_transition(StateTransition::BalanceTransfer {
            from: user_a,
            to: user_b,
            amount: Balance(100_000),
            nonce: Some(0),
            current_block: fork_height + 1,
        })
        .unwrap();
    extend_chain(&mut chain, 3, 100);

    // Verify local state reflects the transfer.
    assert_eq!(
        chain.state.accounts.get(&user_a).unwrap().balance,
        Balance(900_000)
    );
    assert_eq!(
        chain.state.accounts.get(&user_b).unwrap().balance,
        Balance(600_000)
    );

    // Build a longer candidate fork (5 blocks) that does NOT contain the transfer.
    let candidate = build_fork_blocks(fork_tip, fork_height + 1, 5, 200);

    let plan = chain.plan_reorg(&candidate).unwrap().unwrap();
    chain.execute_reorg(&plan, fork_state).unwrap();

    // After reorg, balances should be back to fork-point values.
    assert_eq!(
        chain.state.accounts.get(&user_a).unwrap().balance,
        Balance(1_000_000),
        "User A balance should revert after reorg"
    );
    assert_eq!(
        chain.state.accounts.get(&user_b).unwrap().balance,
        Balance(500_000),
        "User B balance should revert after reorg"
    );

    println!(
        "[#967 PASS] reorg_reverts_balance_transfers: balances correctly reverted"
    );
}

// -------------------------------------------------------------------
// Test 2: Fresh chain matches post-reorg chain (consensus determinism)
// -------------------------------------------------------------------

#[test]
fn fresh_chain_matches_post_reorg() {
    let user = addr(10);

    // Chain A: 5 common blocks, then 3 on orphan fork, then reorg to 5-block candidate.
    let mut chain_a = Blockchain::default();
    chain_a.state.create_account(user, Balance(0));
    extend_chain(&mut chain_a, 5, 0);

    let fork_state = chain_a.state.clone();
    let fork_tip = chain_a.tip;
    let fork_height = chain_a.height;

    extend_chain(&mut chain_a, 3, 100);

    let candidate = build_fork_blocks(fork_tip, fork_height + 1, 5, 200);

    let plan = chain_a.plan_reorg(&candidate).unwrap().unwrap();
    chain_a.execute_reorg(&plan, fork_state.clone()).unwrap();

    // Chain B: 5 common blocks + same candidate blocks directly (never saw orphan fork).
    let mut chain_b = Blockchain::default();
    chain_b.state.create_account(user, Balance(0));
    extend_chain(&mut chain_b, 5, 0);

    // chain_b at fork point should be in same state as fork_state.
    // Apply the candidate blocks directly.
    for block in &candidate {
        chain_b.add_block(block.clone()).unwrap();
    }

    // Both chains must agree on height, tip, and state root.
    assert_eq!(chain_a.height, chain_b.height, "Heights must match");
    assert_eq!(chain_a.tip, chain_b.tip, "Tips must match");

    // Compare state roots (determinism).
    let mut state_a = chain_a.state.clone();
    let mut state_b = chain_b.state.clone();
    state_a.recalculate_state_root();
    state_b.recalculate_state_root();
    assert_eq!(
        state_a.state_root, state_b.state_root,
        "State roots must be identical: reorged chain vs fresh chain"
    );

    println!(
        "[#967 PASS] fresh_chain_matches_post_reorg: state root determinism verified"
    );
}

// -------------------------------------------------------------------
// Test 3: Deep reorg at exactly MAX_REORG_DEPTH succeeds
// -------------------------------------------------------------------

#[test]
fn deep_reorg_at_max_depth_succeeds() {
    let mut chain = Blockchain::default();
    // Build enough blocks so we can reorg exactly MAX_REORG_DEPTH.
    let total_common = 5u64;
    extend_chain(&mut chain, total_common, 0);

    let fork_state = chain.state.clone();
    let fork_tip = chain.tip;
    let fork_height = chain.height;

    // Add exactly MAX_REORG_DEPTH blocks on the local fork.
    extend_chain(&mut chain, MAX_REORG_DEPTH, 1000);
    assert_eq!(chain.height, total_common + MAX_REORG_DEPTH);

    // Candidate fork is 1 block longer than local from the same fork point.
    let candidate = build_fork_blocks(
        fork_tip,
        fork_height + 1,
        MAX_REORG_DEPTH + 1,
        5000,
    );

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(
        plan.is_some(),
        "Reorg at exactly MAX_REORG_DEPTH should be accepted"
    );
    let plan = plan.unwrap();
    assert_eq!(plan.blocks_to_revert, MAX_REORG_DEPTH);

    chain.execute_reorg(&plan, fork_state).unwrap();
    assert_eq!(
        chain.height,
        total_common + MAX_REORG_DEPTH + 1,
        "Chain should be at fork_height + candidate length"
    );

    println!(
        "[#967 PASS] deep_reorg_at_max_depth_succeeds: reorg of {} blocks accepted",
        MAX_REORG_DEPTH
    );
}

// -------------------------------------------------------------------
// Test 4: Deep reorg one past MAX_REORG_DEPTH is rejected
// -------------------------------------------------------------------

#[test]
fn deep_reorg_past_max_depth_rejected() {
    let mut chain = Blockchain::default();
    let total_common = 5u64;
    extend_chain(&mut chain, total_common, 0);

    let fork_tip = chain.tip;
    let fork_height = chain.height;

    // Add MAX_REORG_DEPTH + 1 blocks (one past the limit).
    extend_chain(&mut chain, MAX_REORG_DEPTH + 1, 1000);

    // Candidate is longer.
    let candidate = build_fork_blocks(
        fork_tip,
        fork_height + 1,
        MAX_REORG_DEPTH + 2,
        6000,
    );

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(
        plan.is_none(),
        "Reorg deeper than MAX_REORG_DEPTH should be rejected"
    );

    println!(
        "[#967 PASS] deep_reorg_past_max_depth_rejected: {} + 1 blocks correctly rejected",
        MAX_REORG_DEPTH
    );
}

// -------------------------------------------------------------------
// Test 5: Sequential reorgs converge correctly
// -------------------------------------------------------------------

#[test]
fn sequential_reorgs_converge() {
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);

    // First reorg: fork at 8, candidate 4 blocks (tip at 12).
    let fork_parent_8 = chain.block_at_height(8).unwrap();
    let fork_hash_8 = fork_parent_8.hash.unwrap();
    let fork_state_8 = chain.state.clone();
    let candidate_1 = build_fork_blocks(fork_hash_8, 9, 4, 2000);

    let plan_1 = chain.plan_reorg(&candidate_1).unwrap().unwrap();
    chain.execute_reorg(&plan_1, fork_state_8.clone()).unwrap();
    assert_eq!(chain.height, 12);

    // Second reorg: fork at 10, candidate 5 blocks (tip at 15).
    let fork_parent_10 = chain.block_at_height(10).unwrap();
    let fork_hash_10 = fork_parent_10.hash.unwrap();
    let fork_state_10 = chain.state.clone();
    let candidate_2 = build_fork_blocks(fork_hash_10, 11, 5, 3000);

    let plan_2 = chain.plan_reorg(&candidate_2).unwrap().unwrap();
    chain.execute_reorg(&plan_2, fork_state_10).unwrap();
    assert_eq!(chain.height, 15);

    // Verify blocks at heights 11-15 are from the second candidate.
    for blk in &candidate_2 {
        let in_chain = chain.block_at_height(blk.height).unwrap();
        assert_eq!(
            in_chain.hash.unwrap(),
            blk.hash.unwrap(),
            "Block at height {} should match second candidate after sequential reorg",
            blk.height
        );
    }

    println!(
        "[#967 PASS] sequential_reorgs_converge: two successive reorgs produce correct chain"
    );
}

// -------------------------------------------------------------------
// Test 6: Out-of-order candidate blocks are handled
//
// plan_reorg requires sorted ascending blocks. This test verifies that
// an unsorted candidate is correctly rejected (returns None) rather than
// producing a corrupt plan.
// -------------------------------------------------------------------

#[test]
fn out_of_order_candidate_rejected() {
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);

    let fork_parent = chain.block_at_height(8).unwrap();
    let fork_hash = fork_parent.hash.unwrap();

    // Build 3 valid fork blocks then reverse them.
    let mut candidate = build_fork_blocks(fork_hash, 9, 3, 4000);
    candidate.reverse();

    let plan = chain.plan_reorg(&candidate).unwrap();
    assert!(
        plan.is_none(),
        "Out-of-order candidate blocks should be rejected"
    );

    println!(
        "[#967 PASS] out_of_order_candidate_rejected: reversed candidate correctly rejected"
    );
}

// -------------------------------------------------------------------
// Test 7: State transitions in orphaned fork do not leak
// -------------------------------------------------------------------

#[test]
fn orphaned_state_transitions_do_not_leak() {
    let user_a = addr(20);

    let mut chain = Blockchain::default();
    chain.state.create_account(user_a, Balance(0));
    extend_chain(&mut chain, 5, 0);

    let fork_state = chain.state.clone();
    let fork_tip = chain.tip;
    let fork_height = chain.height;

    // Apply a reward on the local fork.
    chain
        .apply_state_transition(StateTransition::ApplyReward {
            address: user_a,
            amount: Balance(50_000),
            reason: "orphan-reward".to_string(),
        })
        .unwrap();
    extend_chain(&mut chain, 3, 100);

    // Verify reward is in local state.
    assert_eq!(
        chain.state.accounts.get(&user_a).unwrap().balance,
        Balance(50_000)
    );

    // Reorg to a longer fork without the reward.
    let candidate = build_fork_blocks(fork_tip, fork_height + 1, 5, 500);
    let plan = chain.plan_reorg(&candidate).unwrap().unwrap();
    chain.execute_reorg(&plan, fork_state).unwrap();

    // The reward must not persist.
    assert_eq!(
        chain.state.accounts.get(&user_a).unwrap().balance,
        Balance(0),
        "Orphaned reward must not leak into post-reorg state"
    );

    println!(
        "[#967 PASS] orphaned_state_transitions_do_not_leak: reward correctly reverted"
    );
}

// -------------------------------------------------------------------
// Test 8: Commit-reveal interaction with reorgs (#967 + #951 crossover)
//
// If a commit was registered in the orphaned fork, after reorg the
// commit must no longer be present in state.
// -------------------------------------------------------------------

#[test]
fn commit_in_orphaned_fork_reverted_by_reorg() {
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 5, 0);

    let fork_state = chain.state.clone();
    let fork_tip = chain.tip;
    let fork_height = chain.height;

    // Register a commit on the local fork.
    let (commit, _reveal) =
        build_commit_reveal("station-orphan", "orphan-cr", 1_700_000_100);
    chain
        .apply_state_transition(StateTransition::RegisterCommit {
            payload: commit.clone(),
            block_height: fork_height + 1,
        })
        .unwrap();
    extend_chain(&mut chain, 3, 100);

    // Verify commit is pending.
    assert!(
        chain.state.pending_commits.contains_key(&commit.commit_id),
        "Commit should be pending before reorg"
    );

    // Reorg to a longer fork.
    let candidate = build_fork_blocks(fork_tip, fork_height + 1, 5, 700);
    let plan = chain.plan_reorg(&candidate).unwrap().unwrap();
    chain.execute_reorg(&plan, fork_state).unwrap();

    // Commit must be gone.
    assert!(
        !chain.state.pending_commits.contains_key(&commit.commit_id),
        "Commit from orphaned fork must not survive reorg"
    );

    println!(
        "[#967 PASS] commit_in_orphaned_fork_reverted_by_reorg: commit-reveal state correctly reverted"
    );
}

// -------------------------------------------------------------------
// Test 9: Reorg past commit expiry height invalidates the commit
//
// If a commit was registered at height H on the common prefix and the
// reorg moves the chain past H + commit_window, the commit should be
// expired when the new fork is applied.
// -------------------------------------------------------------------

#[test]
fn reorg_past_commit_expiry_invalidates() {
    let commit_height = 100_000u64;
    let cw = commit_window_blocks(commit_height); // 6

    let mut chain = Blockchain::default();
    // Build up to commit_height.
    extend_chain(&mut chain, commit_height, 0);

    // Register commit at commit_height.
    let (commit, reveal) =
        build_commit_reveal("station-expiry-reorg", "expr-1", 1_700_000_200);
    chain
        .apply_state_transition(StateTransition::RegisterCommit {
            payload: commit.clone(),
            block_height: commit_height,
        })
        .unwrap();

    // Snapshot for reorg.
    let fork_state = chain.state.clone();
    let fork_tip = chain.tip;
    let fork_height = chain.height;

    // Extend local chain by 2 (still within commit window).
    extend_chain(&mut chain, 2, 1_000_000);

    // Build a candidate fork that extends past the commit window.
    let candidate_len = cw + 2; // fork_height + cw + 2 > commit_expires_at
    let candidate = build_fork_blocks(fork_tip, fork_height + 1, candidate_len, 2_000_000);

    let plan = chain.plan_reorg(&candidate).unwrap().unwrap();
    chain.execute_reorg(&plan, fork_state).unwrap();

    // Now at height fork_height + candidate_len.
    // The commit was made at commit_height, so commit_expires_at = commit_height + cw.
    // Chain is now at commit_height + cw + 2, past expiry.
    // Attempting to reveal should fail with CommitExpired.
    let result = chain.state.apply_transition(StateTransition::ApplyReveal {
        payload: reveal,
        block_height: chain.height,
        block_timestamp: 0,
    });

    assert!(result.is_err(), "Reveal past commit expiry should fail after reorg");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("commit has expired"),
        "Expected CommitExpired error, got: {err}"
    );

    println!(
        "[#967 PASS] reorg_past_commit_expiry_invalidates: commit correctly expired post-reorg at height {}",
        chain.height
    );
}

// -------------------------------------------------------------------
// Test 10: Single-block reorg is handled
// -------------------------------------------------------------------

#[test]
fn single_block_reorg() {
    let mut chain = Blockchain::default();
    extend_chain(&mut chain, 10, 0);

    let fork_parent = chain.block_at_height(9).unwrap();
    let fork_hash = fork_parent.hash.unwrap();
    let fork_state = chain.state.clone();

    // Candidate: 2 blocks from height 10 (replaces 1 local block with 2).
    let candidate = build_fork_blocks(fork_hash, 10, 2, 8000);

    let plan = chain.plan_reorg(&candidate).unwrap().unwrap();
    assert_eq!(plan.blocks_to_revert, 1);
    assert_eq!(plan.blocks_to_apply.len(), 2);

    chain.execute_reorg(&plan, fork_state).unwrap();
    assert_eq!(chain.height, 11);

    println!(
        "[#967 PASS] single_block_reorg: 1-block reorg to 2-block fork succeeded"
    );
}
