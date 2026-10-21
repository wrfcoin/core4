#![no_main]

//! Fuzz target for layer2 ZK proof deserialization + digest derivation (#1389).
//!
//! `ZKProof::from_bytes` parses a custom binary format that embeds three
//! length-prefixed byte vectors. An operator-submitted L2 proof reaches this
//! path; panics on malformed input are a DoS vector. `derive_proof_digest`
//! hashes the public inputs + verification key and must also be total.
//!
//! We do not call the async `ZKVerifier::verify` here — it requires a Tokio
//! runtime and the internal 5ms sleep would cripple throughput. The
//! non-async surface (from_bytes, to_bytes, digest derivation) covers the
//! pure-parsing attack surface that sees the most untrusted input.

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::layer2::proof::{ZKProof, derive_proof_digest};

fuzz_target!(|data: &[u8]| {
    // Total-ness: parse must never panic on arbitrary bytes.
    if let Ok(proof) = ZKProof::from_bytes(data) {
        // Roundtrip: re-serialize and re-parse must succeed and match.
        let reserialized = proof.to_bytes();
        let _ = ZKProof::from_bytes(&reserialized);

        // Digest derivation must be total on the parsed fields.
        let _ = derive_proof_digest(&proof.public_inputs, &proof.verification_key);
    }

    // Also exercise digest derivation on two arbitrary splits of raw input.
    let mid = data.len() / 2;
    let (inputs, vk) = data.split_at(mid);
    let _ = derive_proof_digest(inputs, vk);
});
