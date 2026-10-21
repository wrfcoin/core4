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
//! ```
//!
//! The chain's environment identity (`WRFCOIN_CHAIN_ID` / `WRFCOIN_NETWORK_ID`
//! and, where registrations are replayed, `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS`)
//! must match the chain being replayed — signature verification during
//! extraction reads the same process configuration a node does.
//!
//! Exit codes: 0 = Verified, 1 = DIVERGED (first divergent height printed),
//! 2 = UNVERIFIED (nothing asserted), 3 = VERIFIED_BY_CHECKPOINT (v1 anchor evidence).

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

use replay_core::{ReplayVerdict, replay_persisted_chain_with_checkpoint_observer};
use wrfcoin_native_chain::storage::Storage;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(db_path) = args.next() else {
        eprintln!("usage: replay_verify <path/to/blocks.redb> [up_to_height]");
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

    let mut checkpoint_assertions = 0u64;
    let verdict = replay_persisted_chain_with_checkpoint_observer(&storage, up_to, |anchor| {
        checkpoint_assertions += 1;
        let outcome = if anchor.matched { "MATCH" } else { "MISMATCH" };
        println!(
            "checkpoint_height={} checkpoint_root={} replayed_root={} {outcome}",
            anchor.height, anchor.checkpoint_root, anchor.replayed_root,
        );
    });
    println!("checkpoints_asserted={checkpoint_assertions}");
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
    std::process::exit(verdict.exit_code());
}
