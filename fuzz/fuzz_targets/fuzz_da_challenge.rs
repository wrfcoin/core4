#![no_main]

//! Fuzz target for the data-availability challenge decode + verify path
//! (#1981).
//!
//! `DataAvailabilityChallenge` and `DataAvailabilityChallengeResponse`
//! (`p2p/src/da_challenge.rs`) travel between storage keepers and challengers
//! and are deserialized from untrusted peer bytes. The response carries a
//! variable-length `data_fragment: Vec<u8>` — a bincode allocation-bomb
//! candidate — and the proof field is verified with `compute_proof` /
//! `verify_challenge_response`.
//!
//! This target exercises:
//!   1. bincode deserialization of both wire types on the raw fuzz bytes.
//!   2. The real verifier `verify_challenge_response(challenge, response,
//!      expected_fragment)` once both decode, plus the standalone
//!      `compute_proof` blake3 path.
//!
//! Run (requires cargo-fuzz / nightly):
//!   cargo +nightly fuzz run fuzz_da_challenge -- -max_total_time=60
//!
//! Must never panic for any byte sequence — all errors must be `Err(_)`.

use libfuzzer_sys::fuzz_target;
use wrfcoin_p2p::da_challenge::{
    DataAvailabilityChallenge,
    DataAvailabilityChallengeResponse,
    compute_proof,
    verify_challenge_response,
};

fuzz_target!(|data: &[u8]| {
    // Both wire types are deserialized from untrusted peer bytes.
    let challenge: Result<DataAvailabilityChallenge, _> = bincode::deserialize(data);
    let response: Result<DataAvailabilityChallengeResponse, _> = bincode::deserialize(data);

    // The proof helper takes a fixed-size nonce and arbitrary fragment bytes;
    // exercise it directly on the raw input.
    let nonce = [0u8; 32];
    let _ = compute_proof(&nonce, data);

    // When both decode, run the full verifier against the decoded fragment.
    if let (Ok(challenge), Ok(response)) = (challenge, response) {
        let expected = response.data_fragment.clone();
        let _ = verify_challenge_response(&challenge, &response, &expected);
        // Also verify against an empty/mismatched fragment to hit the
        // FragmentMismatch and ProofInvalid branches.
        let _ = verify_challenge_response(&challenge, &response, &[]);
    }
});
