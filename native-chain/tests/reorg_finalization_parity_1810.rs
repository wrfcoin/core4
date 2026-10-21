//! #1810 regression — reorg/checkpoint-persist must finalize before checkpointing.
//!
//! The live reorg path finalizes each re-applied block
//! (`State::apply_block_finalization`: deferred-slashing flush, commit/reveal
//! expiry prune, replay-set prune, then `recalculate_state_root`) before
//! capturing a durable checkpoint. The reorg **persist** path previously skipped
//! that finalization, so the checkpoint it wrote embedded *pre-finalization*
//! state. On restart `Storage::load_state` trusts the checkpoint (it skips WAL
//! entries `<= checkpoint height`), so the node reconstructs a `state_root` that
//! disagrees with peers — divergence latch / wedged sync (same class as #1776).
//!
//! This test drives the exact invariant the fix wires into `try_reorg`'s
//! fork-point rebuild and persist loop, via the real `Storage` checkpoint +
//! restart (`load_state`) round-trip. Fully reconstructing a PoP-valid fork
//! through `try_reorg` is impractical in a unit test, so this isolates the
//! finalize-before-checkpoint mechanism the fix routes the reorg path through.

use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::CommitPayload;

fn tmp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("wrfcoin-1810-{}.redb", fastrand::u64(..)))
}

/// Fresh chain whose state holds a single commit registered at `commit_height`.
/// The commit lives in `pending_commits` (a `state_root`-hashed collection)
/// until its reveal window expires, at which point `apply_block_finalization`
/// prunes it.
fn chain_with_pending_commit(commit_height: u64, commit_id: &str) -> Blockchain {
    let mut bc = Blockchain::default();
    bc.apply_state_transition(StateTransition::RegisterCommit {
        payload: CommitPayload {
            commit_id: commit_id.to_string(),
            station_id: "station-1810".to_string(),
            timestamp: 1_700_000_000,
            contribution_type: "contribution".to_string(),
            auth_pubkey: None,
            auth_signature: None,
        },
        block_height: commit_height,
    })
    .expect("RegisterCommit should apply");
    assert!(
        bc.state.pending_commits.contains_key(commit_id),
        "commit must be pending after registration"
    );
    bc
}

/// Persist exactly one block carrying `state` as a durable checkpoint at `hc`,
/// then re-open a fresh `Storage` and reconstruct the chain via `load_state`
/// (the restart path). Returns the reloaded `state_root`.
async fn checkpoint_and_reload_root(
    hc: u64,
    tip: wrfcoin_shared_protocol::crypto::hash::Hash,
    state: &wrfcoin_shared_protocol::blockchain::state::State,
    interval: u64,
) -> [u8; 32] {
    let path = tmp_db();
    {
        let mut st = Storage::open(&path, interval).expect("open storage");
        let mut blk = Block::new(1, tip, vec![], 1, hc);
        let hash = blk.calculate_hash();
        blk.set_hash(hash);
        let checkpoint = st
            .checkpoint_state_bytes(hc, state)
            .expect("checkpoint serialization");
        assert!(
            checkpoint.is_some(),
            "Hc={hc} must be a checkpoint-interval boundary (interval={interval})"
        );
        st.persist_block(blk, vec![], checkpoint)
            .await
            .expect("persist checkpoint block");
    }
    let reloaded = Storage::open(&path, interval)
        .expect("re-open storage")
        .load_state()
        .expect("load_state")
        .expect("persisted state present");
    let _ = std::fs::remove_file(&path);
    reloaded.state.state_root.0
}

#[tokio::test]
async fn reorg_persist_finalized_checkpoint_keeps_restart_state_root_consistent_1810() {
    const INTERVAL: u64 = 4; // small checkpoint interval for the test
    const COMMIT_ID: &str = "1810-detector-commit";
    let commit_height = 2;

    // Discover when the seeded commit's reveal window expires, then pick Hc as a
    // checkpoint-interval boundary safely past expiry so finalization prunes it.
    let probe = chain_with_pending_commit(commit_height, COMMIT_ID);
    let expires_at = probe
        .state
        .pending_commits
        .get(COMMIT_ID)
        .expect("seeded commit present")
        .reveal_expires_at;
    let hc = ((expires_at / INTERVAL) + 2) * INTERVAL;
    assert!(hc % INTERVAL == 0 && hc > expires_at);

    // --- Finalized (correct) state: what the producer + WAL-replay reconstruct.
    let mut finalized = chain_with_pending_commit(commit_height, COMMIT_ID);
    finalized.state.apply_block_finalization(hc);
    let finalized_root = finalized.state.state_root.0;
    assert!(
        !finalized.state.pending_commits.contains_key(COMMIT_ID),
        "finalization at Hc must prune the expired commit (mutates state_root-hashed state)"
    );

    // --- Pre-finalization (buggy) state: only recalculate_state_root, as the
    //     reorg persist loop did before the fix.
    let mut prefinal = chain_with_pending_commit(commit_height, COMMIT_ID);
    prefinal.state.recalculate_state_root();
    let prefinal_root = prefinal.state.state_root.0;
    assert!(
        prefinal.state.pending_commits.contains_key(COMMIT_ID),
        "without finalization the expired commit lingers in state"
    );
    assert_ne!(
        finalized_root, prefinal_root,
        "#1810: a pre-finalization checkpoint root must differ from the finalized root"
    );

    // --- FIX behaviour: a FINALIZED checkpoint at Hc → restart reconstructs the
    //     live finalized root (identical to the non-reorg / WAL-replay path).
    let reloaded_fixed =
        checkpoint_and_reload_root(hc, finalized.tip, &finalized.state, INTERVAL).await;
    assert_eq!(
        reloaded_fixed, finalized_root,
        "restart from a FINALIZED reorg checkpoint must match the live finalized state_root"
    );

    // --- BUG behaviour (guards against regression): a NON-finalized checkpoint
    //     at Hc wedges the restarted node onto the divergent pre-finalization
    //     root, proving the restart trusts the checkpoint and that finalization
    //     before checkpointing is load-bearing.
    let reloaded_buggy =
        checkpoint_and_reload_root(hc, prefinal.tip, &prefinal.state, INTERVAL).await;
    assert_eq!(
        reloaded_buggy, prefinal_root,
        "restart adopts whatever the checkpoint embedded (skips WAL <= Hc)"
    );
    assert_ne!(
        reloaded_buggy, finalized_root,
        "#1810: a pre-finalization reorg checkpoint reconstructs a divergent state_root on restart"
    );
}
