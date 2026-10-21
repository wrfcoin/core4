#![no_main]

//! Fuzz target for snapshot sync payload signature verification.
//!
//! `SignedSnapshotSyncPayload` is deserialized from untrusted P2P messages
//! during snapshot synchronization.  The signature parsing code handles
//! hex-encoded ed25519 public keys and signatures — any panic on malformed
//! input is a denial-of-service vector.

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::blockchain::snapshot_sync::SignedSnapshotSyncPayload;

fuzz_target!(|data: &[u8]| {
    if let Ok(payload) = serde_json::from_slice::<SignedSnapshotSyncPayload>(data) {
        // Exercise signature verification — must never panic.
        let _ = payload.has_valid_signature();
        let _ = payload.canonical_signing_message();
    }
});
