//! Fuzz target for WrfcoinMessage bincode deserialization (#946).
//!
//! `WrfcoinMessage::from_bytes()` is called on every raw gossipsub payload
//! received from untrusted peers.  Bincode with variable-length Vec fields is
//! a known source of allocation bombs and panics on malformed input.
//!
//! Run (requires cargo-fuzz / nightly):
//!   cargo fuzz run fuzz_p2p_message -- -max_total_time=60
//!
//! The target must never panic for any byte sequence — it may only return
//! `Ok(_)` or `Err(_)`.

#![no_main]
use libfuzzer_sys::fuzz_target;
use wrfcoin_p2p::message::WrfcoinMessage;

fuzz_target!(|data: &[u8]| {
    // Must not panic; errors are expected and acceptable for malformed input.
    let _ = WrfcoinMessage::from_bytes(data);
});
