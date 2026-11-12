//! `replay_verify` — replay a persisted chain through THIS binary's follower
//! path and assert every committed state root (#2750). See `replay_core.rs`
//! for what a verdict does and does not prove.
//!
//! Usage:
//!
//! ```bash
//! # Replay a COPY of a node data directory's block DB. Never point this at a
//! # live node's file — redb is single-writer.
//! replay_verify /path/to/copy/of/blocks.redb            # full replay
//! replay_verify /path/to/copy/of/blocks.redb 851        # stop after height 851
//! replay_verify /path/to/copy/of/blocks.redb --compare-metadata
//! ```
//!
//! `--compare-metadata` runs the same replay and, before each block, derives
//! the block's producer-written metadata (governance transitions, reward
//! entries, oracle-local accuracy entries) from the block's transactions and
//! the state before it, exactly as a validating follower does. A block whose
//! metadata differs is printed and counted, not rejected: the replay still
//! applies what the block carries. Zero differences means a follower that
//! enforces the comparison accepts every replayed block with the same state.
//!
//! The same mode also reports, as counts and heights only:
//! - how many replayed blocks carry each of the three metadata keys, so a
//!   clean run can be seen to have covered real reward and governance blocks;
//! - for every block with governance transitions, whether applying them
//!   before or after the block's other transitions reaches the same state
//!   root (`governance_order` lines). The producer applies governance first
//!   and the follower's extractor applies it after the launch transitions;
//!   this shows whether that difference has ever mattered on this chain. Each
//!   line also says whether the follower's order reproduces the state root
//!   the block commits to, and gives the error text when an order fails;
//! - the wall time of the whole run.
//!
//! The chain's environment identity (`WRFCOIN_CHAIN_ID` / `WRFCOIN_NETWORK_ID`
//! and, where registrations are replayed, `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS`)
//! must match the chain being replayed — signature verification during
//! extraction reads the same process configuration a node does.
//!
//! Exit codes: 0 = Verified, 1 = DIVERGED (first divergent height printed),
//! 2 = UNVERIFIED (nothing asserted), 3 = VERIFIED_BY_CHECKPOINT (v1 anchor evidence),
//! 4 = `--compare-metadata` found at least one differing block in a replay
//! that otherwise ended 0 or 3.

#[allow(dead_code)]
#[path = "../launch_api/transition_extractor.rs"]
mod transition_extractor;

// transition_extractor's tests take the process-env lock at
// `crate::helpers::test_env::lock()` (the #2290/#2398 contract). Mount the
// same lock file at the same path in this bin crate so those tests compile
// and serialize their env writes here too.
#[cfg(test)]
#[path = "../launch_api"]
mod helpers {
    pub(crate) mod test_env;
}

mod replay_core;

use replay_core::{
    ReplayVerdict, probe_governance_order, replay_persisted_chain_with_block_precheck,
};
use wrfcoin_native_chain::storage::Storage;

fn main() {
    let mut compare_metadata = false;
    let positional: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| {
            let flag = arg == "--compare-metadata";
            compare_metadata |= flag;
            !flag
        })
        .collect();
    let mut args = positional.into_iter();
    let Some(db_path) = args.next() else {
        eprintln!("usage: replay_verify <path/to/blocks.redb> [up_to_height] [--compare-metadata]");
        std::process::exit(2);
    };
    let up_to = match args.next() {
        None => None,
        Some(raw) => match raw.parse::<u64>() {
            Ok(h) => Some(h),
            Err(_) => {
                eprintln!("up_to_height must be an integer, got '{raw}'");
                std::process::exit(2);
            },
        },
    };

    // Interval only matters for WRITING checkpoints; this tool only reads.
    let storage = match Storage::open(std::path::Path::new(&db_path), 100) {
        Ok(storage) => storage,
        Err(e) => {
            eprintln!("UNVERIFIED: could not open block DB at {db_path}: {e}");
            std::process::exit(2);
        },
    };

    // The node's own consensus construction (`launch_api` main), so the
    // derivation below is the one a validating follower runs.
    let pop_adapter = compare_metadata.then(|| {
        use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType};
        ConsensusFactory::create_pop_adapter(&ConsensusFactory::default_config(
            ConsensusType::ProofOfPublishing,
        ))
    });
    let mut metadata_blocks_compared = 0u64;
    let mut metadata_blocks_differing = 0u64;
    let mut metadata_differences: std::collections::BTreeMap<&'static str, u64> =
        std::collections::BTreeMap::new();

    let mut blocks_with_key: std::collections::BTreeMap<&'static str, u64> =
        std::collections::BTreeMap::new();
    let mut order_blocks = 0u64;
    let mut order_blocks_with_launch_txs = 0u64;
    let mut order_agree = 0u64;
    let mut order_disagree_heights: Vec<u64> = Vec::new();
    let mut order_committed_mismatch_heights: Vec<u64> = Vec::new();
    let started = std::time::Instant::now();

    let mut checkpoint_assertions = 0u64;
    let verdict = replay_persisted_chain_with_block_precheck(
        &storage,
        up_to,
        |state, block| {
            let Some(adapter) = pop_adapter.as_ref() else {
                return;
            };
            metadata_blocks_compared += 1;
            let mismatches = adapter.producer_metadata_mismatches_blocking(state, block);
            if !mismatches.is_empty() {
                metadata_blocks_differing += 1;
            }
            for mismatch in mismatches {
                *metadata_differences.entry(mismatch.code).or_default() += 1;
                println!(
                    "metadata_difference height={} code={} key={} detail={}",
                    block.height, mismatch.code, mismatch.key, mismatch.detail
                );
            }
            for key in ["governance_transitions", "reward_entries", "oracle_local_accuracy_entries"] {
                if block.metadata.get(key).is_some_and(|raw| raw != "[]") {
                    *blocks_with_key.entry(key).or_default() += 1;
                }
            }
            if let Some(probe) = probe_governance_order(state, block) {
                order_blocks += 1;
                if probe.launch_transactions > 0 {
                    order_blocks_with_launch_txs += 1;
                }
                let result = if probe.orders_agree() {
                    order_agree += 1;
                    "same_root"
                } else {
                    order_disagree_heights.push(block.height);
                    match (&probe.launch_first, &probe.governance_first) {
                        (Ok(_), Ok(_)) => "DIFFERENT_ROOT",
                        (Err(_), Ok(_)) => "LAUNCH_FIRST_FAILED",
                        (Ok(_), Err(_)) => "GOVERNANCE_FIRST_FAILED",
                        (Err(_), Err(_)) => "BOTH_FAILED",
                    }
                };
                // The follower's order against the root the block commits to:
                // `match`, `MISMATCH`, or `none` when the block carries no
                // commitment (or the two orders are one list).
                let committed = match probe.launch_first_matches_committed() {
                    Some(true) => "match",
                    Some(false) => {
                        order_committed_mismatch_heights.push(block.height);
                        "MISMATCH"
                    },
                    None => "none",
                };
                // Error text only: what the batch failed with, never state.
                let mut errors = String::new();
                if let Err(error) = &probe.launch_first {
                    errors.push_str(&format!(" launch_first_error={error:?}"));
                }
                if let Err(error) = &probe.governance_first {
                    errors.push_str(&format!(" governance_first_error={error:?}"));
                }
                println!(
                    "governance_order height={} governance_transitions={} launch_txs={} \
                     result={result} launch_first_vs_committed={committed}{errors}",
                    block.height, probe.governance_transitions, probe.launch_transactions
                );
            }
        },
        |anchor| {
            checkpoint_assertions += 1;
            let outcome = if anchor.matched { "MATCH" } else { "MISMATCH" };
            println!(
                "checkpoint_height={} checkpoint_root={} replayed_root={} {outcome}",
                anchor.height, anchor.checkpoint_root, anchor.replayed_root,
            );
        },
    );
    println!("checkpoints_asserted={checkpoint_assertions}");
    if compare_metadata {
        println!(
            "metadata_blocks_compared={metadata_blocks_compared} \
             metadata_blocks_differing={metadata_blocks_differing}"
        );
        for (code, count) in &metadata_differences {
            println!("metadata_differences code={code} count={count}");
        }
        for key in ["governance_transitions", "reward_entries", "oracle_local_accuracy_entries"] {
            println!(
                "metadata_blocks_with_key key={key} count={}",
                blocks_with_key.get(key).copied().unwrap_or(0)
            );
        }
        println!(
            "governance_order_blocks={order_blocks} with_launch_txs={order_blocks_with_launch_txs} \
             same_root={order_agree} not_same_root={} heights={:?}",
            order_disagree_heights.len(),
            order_disagree_heights
        );
        println!(
            "governance_order_launch_first_vs_committed_mismatch={} heights={:?}",
            order_committed_mismatch_heights.len(),
            order_committed_mismatch_heights
        );
        println!("replay_wall_seconds={:.1}", started.elapsed().as_secs_f64());
    }
    match &verdict {
        ReplayVerdict::Verified {
            blocks_replayed,
            roots_asserted,
            checkpoints_asserted,
            first_height,
            last_height,
        } => {
            println!(
                "VERIFIED: replayed {blocks_replayed} block(s) [{first_height}..={last_height}], \
                 {roots_asserted} committed state root(s) reproduced, \
                 {checkpoints_asserted} checkpoint anchor(s) matched"
            );
        },
        ReplayVerdict::VerifiedByCheckpoint {
            blocks_replayed,
            checkpoints_asserted,
            first_height,
            last_height,
            final_replayed_root,
        } => {
            println!(
                "VERIFIED_BY_CHECKPOINT: replayed {blocks_replayed} block(s) \
                 [{first_height}..={last_height}], {checkpoints_asserted} checkpoint anchor(s) \
                 matched, 0 per-block commitments; final_replayed_root={final_replayed_root}"
            );
        },
        ReplayVerdict::Diverged {
            height,
            committed,
            applied,
            blocks_replayed,
        } => {
            eprintln!(
                "DIVERGED at height {height} after {blocks_replayed} clean block(s):\n\
                 \x20 committed: {committed}\n\
                 \x20 applied:   {applied}\n\
                 This binary does not reproduce the historical chain below that height."
            );
        },
        ReplayVerdict::Unverified { reason } => {
            eprintln!("UNVERIFIED: {reason}");
        },
    }
    let code = verdict.exit_code();
    if metadata_blocks_differing > 0 && (code == 0 || code == 3) {
        std::process::exit(4);
    }
    std::process::exit(code);
}
