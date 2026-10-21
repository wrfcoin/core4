#![no_main]

//! Fuzz target for the commit-id computation + commit-payload parse path
//! (#1981).
//!
//! Two untrusted-input entry points in
//! `shared-protocol/src/blockchain/transaction.rs` are exercised:
//!
//!   1. `Transaction::compute_commit_id(data, nonce, station_id, timestamp,
//!      contribution_type)` (transaction.rs:1553) — hashes a JSON `data` value
//!      plus caller-supplied fields. The JSON value originates from the
//!      contributor's commit submission. This must never panic for any JSON
//!      shape or any string field content.
//!
//!   2. `Transaction::parse_commit_payload()` (transaction.rs:1570) — runs
//!      `serde_json::from_str::<CommitPayload>(data)` on a commit/forecast
//!      transaction's `data` field. The transaction itself is deserialized from
//!      the fuzz bytes, mirroring how a peer-supplied transaction reaches the
//!      consensus commit-verification path.
//!
//! Run (requires cargo-fuzz / nightly):
//!   cargo +nightly fuzz run fuzz_commit_id -- -max_total_time=60
//!
//! Must never panic for any byte sequence.

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;

fuzz_target!(|data: &[u8]| {
    // Path 1: compute_commit_id over a JSON value parsed from the fuzz bytes.
    // Any JSON value (object, array, string, number, null) is valid input.
    if let Ok(val) = serde_json::from_slice::<serde_json::Value>(data) {
        // Derive the scalar fields from the raw bytes so the fuzzer can drive
        // arbitrary nonce/station/type strings without a second parse.
        let s = String::from_utf8_lossy(data);
        let _ = Transaction::compute_commit_id(&val, &s, &s, data.len() as i64, &s);
    }

    // Path 2: deserialize a full transaction (as from an untrusted peer) and
    // run the memoized commit-payload parse. The parse only fires for
    // ContributionCommit / ForecastCommit tx types; both branches are reachable
    // depending on the decoded tx_type.
    if let Ok(tx) = serde_json::from_slice::<Transaction>(data) {
        let _ = tx.parse_commit_payload();
    }
});
