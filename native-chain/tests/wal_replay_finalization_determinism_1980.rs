//! #1980 — WAL-replay vs producer `apply_block_finalization` determinism.
//!
//! Two-node determinism is what `state_root` ultimately protects: a producer
//! that mines a block and a follower that reconstructs the same block from WAL
//! on restart MUST land on the identical `state_root`. The
//! producer/applier-finalization bug class (#1776, #1810, #1682) is exactly the
//! failure where the producer mutates `state_root`-hashed state through the
//! per-block finalization trio (deferred-slashing flush, commit/reveal expiry
//! prune, replay-set prune, oracle-reputation decay, then `recalculate_state_root`)
//! via `State::apply_block_finalization`, while an applier/replay path applies the
//! per-tx transition batch but SKIPS that height-driven finalization — so the two
//! nodes silently fork at the first block that crosses a finalization boundary.
//!
//! The existing WAL-replay tests in `storage.rs`
//! (`wal_only_multi_block_replay_recovers_correct_state`, `checkpoint_prunes_wal`,
//! `encrypted_storage_checkpoint_and_wal_replay_work`) only assert recovered
//! HEIGHT and BALANCE — never `state_root` parity, and none cross a commit-reveal
//! expiry boundary, so they cannot catch a WAL-replay path that drops
//! finalization. The State-level (`apply_block_finalization_two_identical_states_…`)
//! and P2P-apply (`producer_applier_commit_reveal_lifecycle_expiry_parity`) tests
//! cover those two sinks; this file closes the WAL-replay-on-restart sink with a
//! producer-vs-replay `state_root` determinism assertion ACROSS an expiry
//! boundary.
//!
//! TESTS ONLY — no production behavior is changed. The tests are expected to PASS
//! on current `main` (where every persistence sink routes through the shared
//! `State::apply_block_finalization`). If the producer-vs-WAL-replay parity test
//! ever FAILS, that is a real cross-node divergence regression, not a flaky test.

use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::common::types::{Address, Balance};
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::CommitPayload;

/// Inert account the per-block reward credits so every non-commit block records
/// a state-hashed WAL entry.
const REWARD_ADDR: Address = Address([0x19; 20]);

fn tmp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("wrfcoin-1980-{}.redb", fastrand::u64(..)))
}

const COMMIT_ID: &str = "1980-finalization-determinism-commit";
const STATION_ID: &str = "station-1980";

/// The per-block `StateTransition` carried in the WAL for block `height`: a
/// `RegisterCommit` exactly at `commit_height`, and an inert (but state-hashed)
/// reward elsewhere so every block records a WAL entry. Returning a single
/// transition per height keeps the producer's per-block batch and the WAL's
/// per-block grouping identical, which is what `Storage::load_state` replays.
fn block_transition(height: u64, commit_height: u64) -> StateTransition {
    if height == commit_height {
        StateTransition::RegisterCommit {
            payload: CommitPayload {
                commit_id: COMMIT_ID.to_string(),
                station_id: STATION_ID.to_string(),
                timestamp: 1_700_000_000,
                contribution_type: "contribution".to_string(),
                auth_pubkey: None,
                auth_signature: None,
            },
            block_height: commit_height,
        }
    } else {
        StateTransition::ApplyReward {
            address: REWARD_ADDR,
            amount: Balance(1),
            reason: format!("1980-block-{height}"),
        }
    }
}

/// Discover the height at which a commit registered at `commit_height` has its
/// reveal window expire, so the test picks a replay span that actually crosses
/// the `apply_block_finalization` prune boundary (rather than hardcoding the
/// governance reveal window).
fn reveal_expiry_height(commit_height: u64) -> u64 {
    let mut probe = Blockchain::default();
    probe
        .apply_state_transition(block_transition(commit_height, commit_height))
        .expect("probe RegisterCommit applies");
    probe
        .state
        .pending_commits
        .get(COMMIT_ID)
        .expect("probe commit is pending")
        .reveal_expires_at
}

/// Build the producer's live state by applying each block's transition batch
/// followed by the shared per-block finalization — exactly what a mining node
/// does — and return the resulting live `State` (carrying the canonical
/// `state_root`).
fn producer_live_state(blocks: u64, commit_height: u64) -> State {
    let mut bc = Blockchain::default();
    bc.state.create_account(REWARD_ADDR, Balance(0));
    for h in 1..=blocks {
        bc.apply_state_transition(block_transition(h, commit_height))
            .expect("producer applies block transition");
        // The mining node finalizes every block through the shared canonical sink.
        bc.state.apply_block_finalization(h);
    }
    bc.state
}

/// THE #1980 INVARIANT: a producer that mines `blocks` blocks across the
/// commit's reveal-expiry boundary, and a follower that reconstructs those
/// blocks from the WAL alone on restart (`Storage::load_state`, no checkpoint),
/// MUST converge on the identical `state_root`.
///
/// The producer finalizes every block via `apply_block_finalization`; restart
/// recovery must apply the SAME height-driven finalization while replaying the
/// WAL, or the expired commit lingers in the replayed `pending_commits` and the
/// roots fork (the #1776 / #1810 divergence class). Asserting on `state_root`
/// (the consensus ground truth) — not just height/balance/block-hash — is what
/// makes this a determinism test.
#[tokio::test]
async fn producer_and_wal_replay_converge_on_state_root_across_expiry_1980() {
    const COMMIT_HEIGHT: u64 = 2;
    // Astronomical interval → no checkpoint is ever written, so recovery is
    // PURE WAL replay from genesis (the path that must re-run finalization).
    const INTERVAL: u64 = u64::MAX / 2;

    let expires_at = reveal_expiry_height(COMMIT_HEIGHT);
    // Mine well past expiry so the producer's finalization prunes the commit and
    // the replay must reproduce that prune to match.
    let blocks = expires_at + 3;

    // --- Producer: live mining state (per-block finalization applied). ---------
    let producer_state = producer_live_state(blocks, COMMIT_HEIGHT);
    assert!(
        !producer_state.pending_commits.contains_key(COMMIT_ID),
        "producer finalization must prune the commit once its reveal window \
         (expires_at={expires_at}) is crossed by height {blocks}"
    );
    let producer_root = producer_state.state_root.0;

    // --- Persist each block with its WAL transition, NO checkpoint. ------------
    let path = tmp_db();
    {
        let mut storage = Storage::open(&path, INTERVAL).expect("open storage");
        let mut bc = Blockchain::default();
        bc.state.create_account(REWARD_ADDR, Balance(0));
        for h in 1..=blocks {
            let transition = block_transition(h, COMMIT_HEIGHT);
            bc.apply_state_transition(transition.clone())
                .expect("persist-side transition applies");
            bc.state.apply_block_finalization(h);

            let mut block = Block::new(1, bc.tip, vec![], 1, h);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            bc.add_block(block.clone()).expect("add_block");

            let checkpoint = storage
                .checkpoint_state_bytes(h, &bc.state)
                .expect("checkpoint_state_bytes");
            assert!(
                checkpoint.is_none(),
                "sanity: interval={INTERVAL} must never produce a checkpoint (got one at h={h})"
            );
            storage
                .persist_block(block, vec![transition], checkpoint)
                .await
                .expect("persist_block");
        }
    }

    // --- Follower restart: recover via WAL replay only. ------------------------
    let reloaded = Storage::open(&path, INTERVAL)
        .expect("re-open storage")
        .load_state()
        .expect("load_state must not error")
        .expect("load_state returns Some after writing blocks");
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        reloaded.height, blocks,
        "WAL replay must recover the full height"
    );
    assert!(
        !reloaded.state.pending_commits.contains_key(COMMIT_ID),
        "#1980: WAL replay must re-run apply_block_finalization and prune the \
         expired commit — otherwise pending_commits (a state_root-hashed \
         collection) lingers and the follower forks from the producer"
    );
    assert_eq!(
        reloaded.state.state_root.0, producer_root,
        "#1980 two-node determinism: producer live state_root and WAL-replay \
         restart state_root must be identical across the commit-reveal expiry \
         boundary (producer/applier finalization parity, #1776/#1810 bug class)"
    );
}

/// NEGATIVE CONTROL — proves the parity test above is actually sensitive to a
/// dropped finalization. A WAL replay that applies the per-tx transitions but
/// SKIPS `apply_block_finalization` (the historical bug shape) reconstructs a
/// DIFFERENT `state_root` once an expired commit lingers. If this assertion ever
/// flips to "equal", the parity assertion in the test above would be vacuous —
/// it would pass even if a future refactor silently dropped finalization.
#[test]
fn skipping_finalization_on_replay_diverges_negative_control_1980() {
    const COMMIT_HEIGHT: u64 = 2;
    let expires_at = reveal_expiry_height(COMMIT_HEIGHT);
    let blocks = expires_at + 3;

    // Correct replay: per-tx batch + per-block finalization (matches producer).
    let finalized_root = {
        let mut s = State::new();
        s.create_account(REWARD_ADDR, Balance(0));
        for h in 1..=blocks {
            s.apply_transition(block_transition(h, COMMIT_HEIGHT))
                .expect("finalized replay transition");
            s.apply_block_finalization(h);
        }
        assert!(
            !s.pending_commits.contains_key(COMMIT_ID),
            "finalized replay must prune the expired commit"
        );
        s.state_root.0
    };

    // Buggy replay: apply transitions, recalculate root, but NEVER finalize.
    let unfinalized_root = {
        let mut s = State::new();
        s.create_account(REWARD_ADDR, Balance(0));
        for h in 1..=blocks {
            s.apply_transition(block_transition(h, COMMIT_HEIGHT))
                .expect("unfinalized replay transition");
        }
        s.recalculate_state_root();
        assert!(
            s.pending_commits.contains_key(COMMIT_ID),
            "without finalization the expired commit must linger in pending_commits"
        );
        s.state_root.0
    };

    assert_ne!(
        finalized_root, unfinalized_root,
        "#1980 sensitivity guard: a replay that skips apply_block_finalization \
         MUST diverge from the finalized root once an expired commit lingers — \
         this is what the producer/WAL-replay parity test detects"
    );
}

/// mobile#215 — a gamification payout must reconstruct IDENTICALLY on
/// WAL-replay-on-restart. `gamification_claims_seen` is a `state_root`-hashed
/// consensus set; if WAL replay did not rebuild it (or credited the payout a
/// second time), the restarted follower would fork from the producer. Proves
/// producer live `state_root` == pure-WAL-replay `state_root`, the recipient is
/// credited exactly once, and the claim id is recorded on the replayed state.
#[tokio::test]
async fn gamification_payout_wal_replay_state_root_parity_215() {
    // Astronomical interval → no checkpoint is ever written, so recovery is
    // PURE WAL replay from genesis (the path that must rebuild the consensus set).
    const INTERVAL: u64 = u64::MAX / 2;
    const BLOCKS: u64 = 3;
    let recipient = Address([0x2a; 20]);
    let claim_id = "wal-claim-215";

    // Height 2 carries the gamification payout; the other heights carry an inert
    // (state-hashed) reward so every block records a WAL entry.
    let tx_at = |h: u64| -> StateTransition {
        if h == 2 {
            StateTransition::ApplyGamificationReward {
                address: recipient,
                amount: Balance(500),
                claim_id: claim_id.to_string(),
                current_block: 2,
            }
        } else {
            StateTransition::ApplyReward {
                address: REWARD_ADDR,
                amount: Balance(1),
                reason: format!("215-block-{h}"),
            }
        }
    };

    // --- Producer: live mining state (per-block finalization applied). ---------
    let mut producer = Blockchain::default();
    producer.state.create_account(REWARD_ADDR, Balance(0));
    for h in 1..=BLOCKS {
        producer
            .apply_state_transition(tx_at(h))
            .expect("producer applies block transition");
        producer.state.apply_block_finalization(h);
    }
    assert_eq!(
        producer.state.get_account(&recipient).map(|a| a.balance.0),
        Some(500),
        "producer credits the payout once"
    );
    assert!(producer.state.gamification_claims_seen.contains(claim_id));
    let producer_root = producer.state.state_root.0;

    // --- Persist each block with its WAL transition, NO checkpoint. ------------
    let path = tmp_db();
    {
        let mut storage = Storage::open(&path, INTERVAL).expect("open storage");
        let mut bc = Blockchain::default();
        bc.state.create_account(REWARD_ADDR, Balance(0));
        for h in 1..=BLOCKS {
            let transition = tx_at(h);
            bc.apply_state_transition(transition.clone())
                .expect("persist-side transition applies");
            bc.state.apply_block_finalization(h);

            let mut block = Block::new(1, bc.tip, vec![], 1, h);
            let hash = block.calculate_hash();
            block.set_hash(hash);
            bc.add_block(block.clone()).expect("add_block");

            let checkpoint = storage
                .checkpoint_state_bytes(h, &bc.state)
                .expect("checkpoint_state_bytes");
            assert!(checkpoint.is_none(), "sanity: no checkpoint at h={h}");
            storage
                .persist_block(block, vec![transition], checkpoint)
                .await
                .expect("persist_block");
        }
    }

    // --- Follower restart: recover via WAL replay only. ------------------------
    let reloaded = Storage::open(&path, INTERVAL)
        .expect("re-open storage")
        .load_state()
        .expect("load_state must not error")
        .expect("load_state returns Some after writing blocks");
    let _ = std::fs::remove_file(&path);

    assert_eq!(reloaded.height, BLOCKS, "WAL replay recovers full height");
    assert!(
        reloaded.state.gamification_claims_seen.contains(claim_id),
        "#215: WAL replay must rebuild the gamification_claims_seen consensus set"
    );
    assert_eq!(
        reloaded.state.get_account(&recipient).map(|a| a.balance.0),
        Some(500),
        "#215: WAL replay must credit the payout exactly once (no double-mint)"
    );
    assert_eq!(
        reloaded.state.state_root.0, producer_root,
        "#215 two-node determinism: producer live state_root and WAL-replay \
         restart state_root must be identical for a gamification payout"
    );
}
