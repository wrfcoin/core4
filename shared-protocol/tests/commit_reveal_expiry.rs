// Commit-reveal window expiry tests (#951).
//
// Verifies that commit-reveal expiry mechanics work correctly:
// - Commits expire after commit_window_blocks
// - Reveal windows expire after reveal_window_blocks
// - Reveals within the window succeed
// - Expired commits are pruned by prune_expired()
// - Window scaling at genesis (14400s blocks → 3-block clamp)
//
// Run from the workspace root:
//   cargo test -p wrfcoin-shared-protocol --test commit_reveal_expiry -- --nocapture

use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{CommitPayload, RevealPayload, Transaction};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    commit_window_blocks, reveal_window_blocks,
};
use wrfcoin_shared_protocol::contributions::commit_reveal::{
    CommitRevealError, CommitRevealState,
};

/// Build a commit/reveal payload pair for a given station, tag, and timestamp.
/// Uses `contribution_type: "contribution"` to avoid weather-specific parsing.
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
// Test 1: Commit expires after commit_window_blocks
// -------------------------------------------------------------------

#[test]
fn commit_expiry_past_deadline() {
    let mut state = State::new();

    // At height 100_000: block_time = 120s, commit_window = 6, reveal_window = 12.
    let commit_height = 100_000u64;
    let cw = commit_window_blocks(commit_height);
    assert_eq!(cw, 6, "sanity: commit window at height 100k");

    let (commit, reveal) =
        build_commit_reveal("station-expiry", "exp-1", 1_700_000_001);

    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit,
            block_height: commit_height,
        })
        .unwrap();

    // Commit is in pending_commits.
    assert!(state.pending_commits.contains_key(&reveal.commit_id));

    // Reveal one block past commit_expires_at → CommitExpired.
    let reveal_height = commit_height + cw + 1;
    let result = state.apply_transition(StateTransition::ApplyReveal {
        payload: reveal,
        block_height: reveal_height,
        block_timestamp: 0,
    });

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("commit has expired"),
        "Expected CommitExpired, got: {err}"
    );

    println!("[#951 PASS] commit_expiry_past_deadline: reveal at height {} (commit_expires_at = {}) correctly rejected",
        reveal_height, commit_height + cw);
}

// -------------------------------------------------------------------
// Test 2: Reveal window expiry
//
// In the current implementation, commit_expires_at ≤ reveal_expires_at
// for all heights (BASE_COMMIT < BASE_REVEAL), so CommitExpired fires
// first when both are exceeded. To exercise the RevealWindowExpired
// path in isolation we test CommitRevealState directly with a widened
// commit deadline.
// -------------------------------------------------------------------

#[test]
fn reveal_window_expiry_direct() {
    let mut cr = CommitRevealState::default();
    let (commit, reveal) =
        build_commit_reveal("station-rwx", "rwx-1", 1_700_000_002);

    // Register commit at height 0 (both windows clamp to 3).
    cr.register_commit(&commit, 0).unwrap();

    // Widen commit_expires_at so CommitExpired does not mask RevealWindowExpired.
    let record = cr
        .pending_commits
        .get_mut(&commit.commit_id)
        .unwrap();
    record.commit_expires_at = u64::MAX;
    record.reveal_expires_at = 5;

    // Reveal at height 6 → past reveal_expires_at but within commit_expires_at.
    let result = cr.apply_reveal(&reveal, 6);
    assert_eq!(
        result.unwrap_err(),
        CommitRevealError::RevealWindowExpired,
        "Should get RevealWindowExpired when past reveal deadline"
    );

    println!("[#951 PASS] reveal_window_expiry_direct: RevealWindowExpired at height 6 (reveal_expires_at = 5)");
}

// -------------------------------------------------------------------
// Test 3: Within-window reveal succeeds
// -------------------------------------------------------------------

#[test]
fn within_window_reveal_succeeds() {
    let mut state = State::new();

    let commit_height = 100_000u64;
    let (commit, reveal) =
        build_commit_reveal("station-ok", "ok-1", 1_700_000_003);

    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit,
            block_height: commit_height,
        })
        .unwrap();

    // Reveal in the very next block → must succeed.
    let result = state.apply_transition(StateTransition::ApplyReveal {
        payload: reveal.clone(),
        block_height: commit_height + 1,
        block_timestamp: 0,
    });

    assert!(
        result.is_ok(),
        "Reveal within window should succeed: {:?}",
        result.err()
    );

    // Commit consumed: moved from pending_commits to pending_reveals.
    assert!(
        !state.pending_commits.contains_key(&reveal.commit_id),
        "Commit should no longer be pending"
    );
    assert!(
        state.pending_reveals.contains_key(&reveal.commit_id),
        "Reveal should be recorded"
    );

    println!("[#951 PASS] within_window_reveal_succeeds: reveal at height {} accepted",
        commit_height + 1);
}

// -------------------------------------------------------------------
// Test 4: Expired commit pruning
// -------------------------------------------------------------------

#[test]
fn expired_commit_pruning() {
    let mut cr = CommitRevealState::default();

    let (commit, reveal) =
        build_commit_reveal("station-prune", "prune-1", 1_700_000_004);

    // Register at height 0. With the wall-clock cap (#1040),
    // reveal_expires_at = 0 + reveal_window_blocks(0) = 0 + 1 = 1.
    cr.register_commit(&commit, 0).unwrap();
    assert!(cr.pending_commits.contains_key(&commit.commit_id));
    let rw = reveal_window_blocks(0);
    let reveal_expires_at = rw; // 0 + rw

    // prune_expired retains records where current_height <= reveal_expires_at.
    // At the boundary → retained.
    cr.prune_expired(reveal_expires_at);
    assert!(
        cr.pending_commits.contains_key(&commit.commit_id),
        "Commit should survive pruning at the boundary (height {})",
        reveal_expires_at
    );

    // One block past the boundary → removed.
    cr.prune_expired(reveal_expires_at + 1);
    assert!(
        !cr.pending_commits.contains_key(&commit.commit_id),
        "Expired commit should be pruned at height {}",
        reveal_expires_at + 1
    );

    // Subsequent reveal must fail with UnknownCommitForReveal.
    let result = cr.apply_reveal(&reveal, reveal_expires_at + 1);
    assert_eq!(
        result.unwrap_err(),
        CommitRevealError::UnknownCommitForReveal,
        "Reveal after pruning should fail with UnknownCommitForReveal"
    );

    println!("[#951 PASS] expired_commit_pruning: commit pruned at height {}, reveal correctly rejected",
        reveal_expires_at + 1);
}

// -------------------------------------------------------------------
// Test 5: Window scaling at genesis
// -------------------------------------------------------------------

#[test]
fn window_scaling_at_genesis() {
    // At genesis (height 0), block_time = 14_400s (4 hours).
    // commit_window = (720 / 14400).max(3) = 0.max(3) = 3
    // reveal_window = (1440 / 14400).max(3) = 0.max(3) = 3
    //
    // The wall-clock cap (#1040) would shrink this toward 1 block, but the
    // 3-block safety floor takes precedence over the cap (commit bf4d53245)
    // so genesis-era windows never collapse below 3 blocks — a commit-reveal
    // pair always has room to land in distinct blocks. So both windows are 3
    // at genesis. (NOTE: at 14400s blocks a 3-block window is 12h wall-clock,
    // which exceeds the 4h `MAX_COMMIT_REVEAL_WALL_CLOCK_SECONDS`; the floor
    // intentionally wins. Flagged for consensus review — see PR #1741.)
    assert_eq!(
        commit_window_blocks(0),
        3,
        "commit_window_blocks(0) should be 3 (3-block safety floor wins over the wall-clock cap)"
    );
    assert_eq!(
        reveal_window_blocks(0),
        3,
        "reveal_window_blocks(0) should be 3 (3-block safety floor wins over the wall-clock cap)"
    );

    // At height 100_000 (block_time = 120s), windows diverge:
    // commit_window = 720 / 120 = 6
    // reveal_window = 1440 / 120 = 12
    assert_eq!(commit_window_blocks(100_000), 6);
    assert_eq!(reveal_window_blocks(100_000), 12);

    // At terminal height (block_time = 1s), full base values:
    // commit_window = 720, reveal_window = 1440
    assert_eq!(commit_window_blocks(10_000_000), 720);
    assert_eq!(reveal_window_blocks(10_000_000), 1440);

    println!("[#951 PASS] window_scaling_at_genesis: all scaling checkpoints verified");
}

// -------------------------------------------------------------------
// Test 6: Reveal exactly at commit_expires_at boundary succeeds
// -------------------------------------------------------------------

#[test]
fn reveal_at_commit_expiry_boundary_succeeds() {
    let mut state = State::new();

    let commit_height = 100_000u64;
    let cw = commit_window_blocks(commit_height); // 6
    let (commit, reveal) =
        build_commit_reveal("station-boundary", "bnd-1", 1_700_000_005);

    state
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit,
            block_height: commit_height,
        })
        .unwrap();

    // Reveal exactly at commit_expires_at (100_006) → not past it → succeeds.
    let reveal_height = commit_height + cw;
    let result = state.apply_transition(StateTransition::ApplyReveal {
        payload: reveal,
        block_height: reveal_height,
        block_timestamp: 0,
    });

    assert!(
        result.is_ok(),
        "Reveal at exact commit_expires_at boundary should succeed: {:?}",
        result.err()
    );

    println!("[#951 PASS] reveal_at_commit_expiry_boundary_succeeds: reveal at height {} (commit_expires_at = {}) accepted",
        reveal_height, reveal_height);
}
