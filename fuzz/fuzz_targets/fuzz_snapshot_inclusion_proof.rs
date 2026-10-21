#![no_main]

//! Fuzz target for snapshot inclusion proof verification (#1389).
//!
//! `SnapshotInclusionProof` is walked by `verify_inclusion_proof` during
//! snapshot sync bootstrap. The walker consumes a `Vec<SnapshotProofNode>`
//! from untrusted peers — an allocation bomb or panic here would DoS a
//! syncing node. The existing `fuzz_snapshot_sync` only exercises the
//! signed-payload signature path, not the inclusion proof walker.

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::blockchain::snapshot_sync::{
    SnapshotInclusionProof, verify_inclusion_proof,
};

fuzz_target!(|data: &[u8]| {
    let Ok(proof) = serde_json::from_slice::<SnapshotInclusionProof>(data) else {
        return;
    };

    // Exercise the walker against arbitrary key/value bytes taken from the
    // input itself — must never panic regardless of siblings vector shape
    // or hex content.
    let (k, v) = data.split_at(data.len() / 2);
    let _ = verify_inclusion_proof(&proof, k, v);
    let _ = verify_inclusion_proof(&proof, &[], &[]);
    let _ = verify_inclusion_proof(&proof, data, data);
});
