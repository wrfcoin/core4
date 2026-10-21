#![no_main]

//! Fuzz target for Groth16 proof canonical deserialization (#1981).
//!
//! `proving::verify` (`zk-circuits/src/proving.rs:64`) decodes an untrusted,
//! caller-supplied proof with
//! `Proof::<Bn254>::deserialize_compressed(&zk_proof.proof_bytes)` before the
//! pairing check. The `proof_bytes` arrive from transaction payloads
//! (confidential transfer / shielded deposit-withdraw / layer2), so a panic in
//! arkworks' compressed point decode on malformed input is a chain-halt /
//! DoS vector reachable from the mempool.
//!
//! This target exercises the exact decode entry point on raw fuzz bytes:
//!   `Proof::<Bn254>::deserialize_compressed`
//! (both the borrowing reader form and `deserialize_uncompressed`, since the
//! attacker controls which length/encoding they present). Any panic — rather
//! than a clean `Err(SerializationError)` — is a bug.
//!
//! Run (requires cargo-fuzz / nightly):
//!   cargo +nightly fuzz run fuzz_groth16_proof_deser -- -max_total_time=60
//!
//! Must never panic for any byte sequence — all errors must be `Err(_)`.

use ark_bn254::Bn254;
use ark_groth16::Proof;
use ark_serialize::CanonicalDeserialize;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The production verify path uses compressed decoding.
    let _ = Proof::<Bn254>::deserialize_compressed(data);
    // Uncompressed decoding shares the curve-point parsing machinery; exercise
    // it too so the fuzzer covers both length expectations.
    let _ = Proof::<Bn254>::deserialize_uncompressed(data);
});
