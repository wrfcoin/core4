#![no_main]

//! Fuzz target for the inner `FinalityVote` bincode decode (#1981).
//!
//! The P2P layer carries a finality vote as an opaque `vote_bytes: Vec<u8>`
//! inside `WrfcoinMessage::FinalityVote`. The launch_api event loop
//! (`native-chain/src/bin/launch_api/p2p_events.rs:1691`) decodes that inner
//! payload with `bincode::deserialize::<FinalityVote>(&vote_bytes)` before
//! handing it to the BFT gadget. The `FinalityVote` struct
//! (`consensus-engines::selection::bft_finality::FinalityVote`) has a
//! variable-length `validator_id: String` and `signature: Vec<u8>`, both
//! attacker-controlled and a bincode allocation-bomb surface.
//!
//! This target exercises:
//!   1. `bincode::deserialize::<FinalityVote>` on the raw fuzz bytes (the exact
//!      inner decode the node performs on the gossip payload).
//!   2. `content_hash()` on any accepted vote — the deterministic dedup hash
//!      the gadget computes — which must never panic.
//!
//! Run (requires cargo-fuzz / nightly):
//!   cargo +nightly fuzz run fuzz_finality_vote_inner -- -max_total_time=60
//!
//! Must never panic for any byte sequence — all errors must be `Err(_)`.

use libfuzzer_sys::fuzz_target;
use wrfcoin_consensus_engines::FinalityVote;

fuzz_target!(|data: &[u8]| {
    // Mirror the node's inner decode of `vote_bytes`.
    if let Ok(vote) = bincode::deserialize::<FinalityVote>(data) {
        // Deterministic content hash used for dedup; must not panic.
        let _ = vote.content_hash();
    }
});
