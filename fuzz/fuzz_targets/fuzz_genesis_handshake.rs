#![no_main]

//! Fuzz target for `GenesisHandshakeMessage` bincode deserialization (#1981).
//!
//! Every peer sends a `GenesisHandshakeMessage` during the P2P handshake
//! (`p2p/src/genesis_handshake.rs`). `from_bytes()` runs bincode over raw,
//! untrusted bytes received before the peer has been authenticated, so a panic
//! or allocation bomb here is a pre-auth remote denial-of-service vector. The
//! struct carries variable-length `String` fields, a classic source of bincode
//! over-allocation on malformed input.
//!
//! This target exercises:
//!   1. `GenesisHandshakeMessage::from_bytes` on the raw fuzz bytes.
//!   2. On a successful decode, the downstream verification path:
//!      `has_placeholder_identity()` and `verify_match()` against a fixed local
//!      handshake — the exact functions the node runs before deciding to keep
//!      or drop the peer.
//!
//! Run (requires cargo-fuzz / nightly):
//!   cargo +nightly fuzz run fuzz_genesis_handshake -- -max_total_time=60
//!
//! Must never panic for any byte sequence — all errors must be `Err(_)`.

use libfuzzer_sys::fuzz_target;
use wrfcoin_p2p::genesis_handshake::GenesisHandshakeMessage;

fuzz_target!(|data: &[u8]| {
    // Decode an untrusted handshake from the wire. Errors are expected.
    if let Ok(remote) = GenesisHandshakeMessage::from_bytes(data) {
        // Exercise the placeholder / mismatch logic that runs on every
        // accepted decode. None of these may panic.
        let _ = remote.has_placeholder_identity();

        let local = GenesisHandshakeMessage::new(
            "testnet".to_string(),
            "wrfcoin-test".to_string(),
            "a3301def00000000000000000000000000000000000000000000000000000000".to_string(),
            1,
        );
        let _ = local.verify_match(&remote);
        // Verify is non-symmetric over placeholder identities; check both ways.
        let _ = remote.verify_match(&local);

        // Round-trip the accepted message; re-serialization must not panic.
        let _ = remote.to_bytes();
    }
});
