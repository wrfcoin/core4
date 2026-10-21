// Network partition recovery & state convergence tests (#725).
//
// Proves that the WRFCoin state layer is deterministic and convergent:
// two nodes that apply the same transitions from the same starting state
// always arrive at the same state root, regardless of insertion order
// (for non-overlapping accounts) or pruning history.
//
// Run from the workspace root:
//   cargo test -p wrfcoin-shared-protocol --test partition_convergence -- --nocapture

use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{CommitPayload, RevealPayload, Transaction};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::contributions::commit_reveal::APPLIED_REVEAL_RETENTION_BLOCKS;

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

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

/// Same transition sequence on two independent states produces identical
/// roots at every step.
#[test]
fn state_root_determinism_same_sequence() {
    let mut state_a = State::new();
    let mut state_b = State::new();

    // Step 1: bootstrap two accounts via rewards.
    state_a
        .apply_transition(StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(10_000),
            reason: "bootstrap".to_string(),
        })
        .unwrap();
    state_b
        .apply_transition(StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(10_000),
            reason: "bootstrap".to_string(),
        })
        .unwrap();
    assert_eq!(state_a.state_root, state_b.state_root);

    // Step 2: second reward.
    state_a
        .apply_transition(StateTransition::ApplyReward {
            address: addr(2),
            amount: Balance::new(5_000),
            reason: "bootstrap".to_string(),
        })
        .unwrap();
    state_b
        .apply_transition(StateTransition::ApplyReward {
            address: addr(2),
            amount: Balance::new(5_000),
            reason: "bootstrap".to_string(),
        })
        .unwrap();
    assert_eq!(state_a.state_root, state_b.state_root);

    // Step 3: balance transfer.
    state_a
        .apply_transition(StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(2),
            amount: Balance::new(1_000),
            nonce: Some(0),
            current_block: 0,
        })
        .unwrap();
    state_b
        .apply_transition(StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(2),
            amount: Balance::new(1_000),
            nonce: Some(0),
            current_block: 0,
        })
        .unwrap();
    assert_eq!(state_a.state_root, state_b.state_root);

    // Step 4: register a commit.
    let (commit, reveal) = build_commit_reveal("stationX", "cr-1", 1_700_000_001);
    state_a
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit.clone(),
            block_height: 1,
        })
        .unwrap();
    state_b
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit,
            block_height: 1,
        })
        .unwrap();
    assert_eq!(state_a.state_root, state_b.state_root);

    // Step 5: apply the reveal.
    state_a
        .apply_transition(StateTransition::ApplyReveal {
            payload: reveal.clone(),
            block_height: 2,
            block_timestamp: 0,
        })
        .unwrap();
    state_b
        .apply_transition(StateTransition::ApplyReveal {
            payload: reveal,
            block_height: 2,
            block_timestamp: 0,
        })
        .unwrap();
    assert_eq!(state_a.state_root, state_b.state_root);

    // Final root must not be all zeros (proves actual hashing occurred).
    assert_ne!(state_a.state_root.0, [0u8; 32]);
}

/// Independent (non-overlapping-account) transitions applied in different
/// orders produce the same state root, because `recalculate_state_root`
/// sorts all entries before hashing.
///
/// NOTE: This only works for non-overlapping senders. Shared-sender
/// transfers are order-dependent due to balance/nonce side effects.
#[test]
fn independent_transitions_order_insensitive() {
    let mut state_a = State::new();
    let mut state_b = State::new();

    // Apply 5 rewards to different addresses in forward vs reverse order.
    let rewards: Vec<(u8, Balance)> = vec![
        (10, Balance::new(1000)),
        (20, Balance::new(2000)),
        (30, Balance::new(3000)),
        (40, Balance::new(4000)),
        (50, Balance::new(5000)),
    ];

    for &(byte, amount) in &rewards {
        state_a
            .apply_transition(StateTransition::ApplyReward {
                address: addr(byte),
                amount,
                reason: "seed".to_string(),
            })
            .unwrap();
    }
    for &(byte, amount) in rewards.iter().rev() {
        state_b
            .apply_transition(StateTransition::ApplyReward {
                address: addr(byte),
                amount,
                reason: "seed".to_string(),
            })
            .unwrap();
    }
    assert_eq!(state_a.state_root, state_b.state_root);

    // Extend with independent transfers (different senders).
    // State A: transfer(10→11, 100), then transfer(20→21, 200)
    // State B: transfer(20→21, 200), then transfer(10→11, 100)
    state_a
        .apply_transition(StateTransition::BalanceTransfer {
            from: addr(10),
            to: addr(11),
            amount: Balance::new(100),
            nonce: Some(0),
            current_block: 0,
        })
        .unwrap();
    state_a
        .apply_transition(StateTransition::BalanceTransfer {
            from: addr(20),
            to: addr(21),
            amount: Balance::new(200),
            nonce: Some(0),
            current_block: 0,
        })
        .unwrap();

    state_b
        .apply_transition(StateTransition::BalanceTransfer {
            from: addr(20),
            to: addr(21),
            amount: Balance::new(200),
            nonce: Some(0),
            current_block: 0,
        })
        .unwrap();
    state_b
        .apply_transition(StateTransition::BalanceTransfer {
            from: addr(10),
            to: addr(11),
            amount: Balance::new(100),
            nonce: Some(0),
            current_block: 0,
        })
        .unwrap();

    assert_eq!(state_a.state_root, state_b.state_root);
}

/// Two nodes diverge from a common fork point, then the shorter-chain
/// node replays the longer chain's transitions and converges to the
/// same state root.
///
/// No `Blockchain` reorg needed — the test proves state-layer determinism:
/// same transitions from same starting state = same result. Real partition
/// recovery (chain selection, block fetching) is a P2P-layer concern
/// built on top of this guarantee.
#[test]
fn partition_convergence_via_state_replay() {
    // Common genesis: bootstrap two accounts.
    let mut common = State::new();
    common
        .apply_transition(StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(50_000),
            reason: "genesis".to_string(),
        })
        .unwrap();
    common
        .apply_transition(StateTransition::ApplyReward {
            address: addr(2),
            amount: Balance::new(50_000),
            reason: "genesis".to_string(),
        })
        .unwrap();

    // 3 shared transitions.
    common
        .apply_transition(StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(2),
            amount: Balance::new(5_000),
            nonce: Some(0),
            current_block: 1,
        })
        .unwrap();
    common
        .apply_transition(StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(2_000),
            reason: "mining".to_string(),
        })
        .unwrap();
    common
        .apply_transition(StateTransition::BalanceTransfer {
            from: addr(2),
            to: addr(1),
            amount: Balance::new(1_000),
            nonce: Some(0),
            current_block: 2,
        })
        .unwrap();

    let fork_point = common.clone();
    let fork_root = fork_point.state_root;
    assert_ne!(fork_root.0, [0u8; 32]);

    // --- Node A: 5 transitions from fork_point ---
    let node_a_transitions = vec![
        StateTransition::ApplyReward {
            address: addr(6),
            amount: Balance::new(10_000),
            reason: "block-A1".to_string(),
        },
        StateTransition::ApplyReward {
            address: addr(7),
            amount: Balance::new(8_000),
            reason: "block-A2".to_string(),
        },
        StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(6),
            amount: Balance::new(3_000),
            nonce: Some(1),
            current_block: 3,
        },
        StateTransition::ApplyReward {
            address: addr(8),
            amount: Balance::new(5_000),
            reason: "block-A4".to_string(),
        },
        StateTransition::BalanceTransfer {
            from: addr(2),
            to: addr(7),
            amount: Balance::new(2_000),
            nonce: Some(1),
            current_block: 4,
        },
    ];

    let mut state_a = fork_point.clone();
    for t in &node_a_transitions {
        state_a.apply_transition(t.clone()).unwrap();
    }

    // --- Node B: 2 transitions from fork_point (shorter chain) ---
    let mut state_b = fork_point.clone();
    state_b
        .apply_transition(StateTransition::ApplyReward {
            address: addr(9),
            amount: Balance::new(7_000),
            reason: "block-B1".to_string(),
        })
        .unwrap();
    state_b
        .apply_transition(StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(9),
            amount: Balance::new(1_500),
            nonce: Some(1),
            current_block: 3,
        })
        .unwrap();

    // Nodes have diverged.
    assert_ne!(state_a.state_root, state_b.state_root);

    // --- Healing: B replays A's transitions from the fork point ---
    let mut state_b_healed = fork_point;
    for t in &node_a_transitions {
        state_b_healed.apply_transition(t.clone()).unwrap();
    }

    // Converged: healed B matches A exactly.
    assert_eq!(state_a.state_root, state_b_healed.state_root);
}

/// After the 10k-block reveal-pruning window, two states that processed
/// identical commit-reveal sequences still converge. A late-joining node
/// replaying the full history also arrives at the same root.
#[test]
fn pruning_does_not_break_convergence() {
    let mut state_a = State::new();
    let mut state_b = State::new();

    // 3 commit-reveal pairs at low heights.
    let pairs = [
        ("station-1", "pr-1", 1_700_000_001i64, 1u64, 2u64),
        ("station-2", "pr-2", 1_700_000_002, 10, 11),
        ("station-3", "pr-3", 1_700_000_003, 100, 101),
    ];

    for &(station, tag, ts, commit_h, reveal_h) in &pairs {
        let (commit, reveal) = build_commit_reveal(station, tag, ts);
        for state in [&mut state_a, &mut state_b] {
            state
                .apply_transition(StateTransition::RegisterCommit {
                    payload: commit.clone(),
                    block_height: commit_h,
                })
                .unwrap();
            state
                .apply_transition(StateTransition::ApplyReveal {
                    payload: reveal.clone(),
                    block_height: reveal_h,
                    block_timestamp: 0,
                })
                .unwrap();
        }
    }

    assert_eq!(state_a.state_root, state_b.state_root);
    let root_before_pruning = state_a.state_root;

    // 4th commit at height APPLIED_REVEAL_RETENTION_BLOCKS + 200 (= 10,200).
    // This triggers prune_expired(10200) which removes reveals with
    // revealed_at < 200 — i.e., all 3 early reveals.
    let pruning_height = APPLIED_REVEAL_RETENTION_BLOCKS + 200;
    let (commit_4, reveal_4) = build_commit_reveal("station-4", "pr-4", 1_700_000_004);

    for state in [&mut state_a, &mut state_b] {
        state
            .apply_transition(StateTransition::RegisterCommit {
                payload: commit_4.clone(),
                block_height: pruning_height,
            })
            .unwrap();
        state
            .apply_transition(StateTransition::ApplyReveal {
                payload: reveal_4.clone(),
                block_height: pruning_height + 1,
                block_timestamp: 0,
            })
            .unwrap();
    }

    // Both states still match after pruning.
    assert_eq!(state_a.state_root, state_b.state_root);
    // Pruning changed internal state, so root differs from before.
    assert_ne!(state_a.state_root, root_before_pruning);

    // Late-joiner: replay all 4 commit-reveal cycles from scratch.
    let mut state_c = State::new();
    for &(station, tag, ts, commit_h, reveal_h) in &pairs {
        let (commit, reveal) = build_commit_reveal(station, tag, ts);
        state_c
            .apply_transition(StateTransition::RegisterCommit {
                payload: commit,
                block_height: commit_h,
            })
            .unwrap();
        state_c
            .apply_transition(StateTransition::ApplyReveal {
                payload: reveal,
                block_height: reveal_h,
                block_timestamp: 0,
            })
            .unwrap();
    }
    state_c
        .apply_transition(StateTransition::RegisterCommit {
            payload: commit_4.clone(),
            block_height: pruning_height,
        })
        .unwrap();
    state_c
        .apply_transition(StateTransition::ApplyReveal {
            payload: reveal_4.clone(),
            block_height: pruning_height + 1,
            block_timestamp: 0,
        })
        .unwrap();

    // Late joiner converges.
    assert_eq!(state_a.state_root, state_c.state_root);
}
