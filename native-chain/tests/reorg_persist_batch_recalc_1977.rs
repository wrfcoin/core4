//! #1977 regression — batched state-root recalc must be byte-identical to the
//! per-entry recalc the reorg-persist loop used before.
//!
//! The reorg persist loop in `launch_api::p2p_events` previously applied each
//! block's transitions with `State::apply_transition` (a full
//! `recalculate_state_root` per transition: O(entries × state-size)), then ran
//! `apply_block_finalization`. The fix replaces the per-entry loop with a single
//! `State::apply_transitions_batch` (one recalc for the whole block) followed by
//! the SAME `apply_block_finalization`.
//!
//! This is consensus-critical: the resulting `state_root` MUST be byte-identical
//! to the per-entry path — a divergent batching would be a fork bug far worse
//! than the perf issue it fixes. These tests pin that equivalence directly on the
//! `State` APIs the substitution swaps, both with and without the trailing
//! per-block finalization, and across multiple blocks (the persist loop is
//! per-block).

use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::CommitPayload;

/// A `RegisterCommit` transition (no signature required) that mutates
/// `state_root`-hashed state (`pending_commits`).
fn commit_transition(commit_id: &str, block_height: u64) -> StateTransition {
    StateTransition::RegisterCommit {
        payload: CommitPayload {
            commit_id: commit_id.to_string(),
            station_id: "station-1977".to_string(),
            timestamp: 1_700_000_000,
            contribution_type: "contribution".to_string(),
            auth_pubkey: None,
            auth_signature: None,
        },
        block_height,
    }
}

/// One "block" worth of transitions.
fn block_transitions(block_height: u64, count: usize) -> Vec<StateTransition> {
    (0..count)
        .map(|i| commit_transition(&format!("1977-h{block_height}-c{i}"), block_height))
        .collect()
}

/// Per-entry path: exactly what the reorg-persist loop did before the fix —
/// `apply_transition` (recalc per entry) for each transition in the block.
fn apply_per_entry(state: &mut State, transitions: &[StateTransition]) {
    for t in transitions {
        state
            .apply_transition(t.clone())
            .expect("per-entry apply_transition should succeed");
    }
}

/// Batched path: exactly what the fix does — one `apply_transitions_batch`
/// (single recalc) for the whole block.
fn apply_batched(state: &mut State, transitions: &[StateTransition]) {
    state
        .apply_transitions_batch(transitions.iter().cloned())
        .expect("batched apply_transitions_batch should succeed");
}

#[test]
fn batched_recalc_matches_per_entry_state_root_single_block_1977() {
    let txs = block_transitions(5, 8);

    let mut per_entry = State::new();
    apply_per_entry(&mut per_entry, &txs);

    let mut batched = State::new();
    apply_batched(&mut batched, &txs);

    assert_eq!(
        per_entry.state_root.0, batched.state_root.0,
        "#1977: batched recalc must produce a byte-identical state_root to the per-entry path"
    );
    assert_eq!(
        per_entry.pending_commits.len(),
        batched.pending_commits.len(),
        "both paths must apply the same transitions"
    );
}

#[test]
fn batched_recalc_matches_per_entry_with_finalization_1977() {
    // Mirror the persist loop's actual shape: apply the block, then run the SAME
    // `apply_block_finalization` on both sides. The final state_root must match.
    const HEIGHT: u64 = 7;
    let txs = block_transitions(HEIGHT, 6);

    let mut per_entry = State::new();
    apply_per_entry(&mut per_entry, &txs);
    per_entry.apply_block_finalization(HEIGHT);

    let mut batched = State::new();
    apply_batched(&mut batched, &txs);
    batched.apply_block_finalization(HEIGHT);

    assert_eq!(
        per_entry.state_root.0, batched.state_root.0,
        "#1977: batched apply + finalization must equal per-entry apply + finalization"
    );
}

#[test]
fn batched_recalc_matches_per_entry_across_multiple_blocks_1977() {
    // The reorg-persist loop is per-block over an evolving state. Replicate that:
    // multiple blocks applied in sequence, finalizing each, on the same state.
    let blocks: Vec<(u64, Vec<StateTransition>)> =
        (1u64..=4).map(|h| (h, block_transitions(h, h as usize + 2))).collect();

    let mut per_entry = State::new();
    for (h, txs) in &blocks {
        apply_per_entry(&mut per_entry, txs);
        per_entry.apply_block_finalization(*h);
    }

    let mut batched = State::new();
    for (h, txs) in &blocks {
        apply_batched(&mut batched, txs);
        batched.apply_block_finalization(*h);
    }

    assert_eq!(
        per_entry.state_root.0, batched.state_root.0,
        "#1977: per-block batched recalc over an evolving state must match the per-entry path"
    );
}
