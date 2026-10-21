//! Data Availability (DA) challenge protocol for the WRFCoin P2P layer.
//!
//! # Overview
//!
//! Weather data blobs (radar sweeps, satellite imagery) are too large for the
//! blockchain state.  Only the content identifier (CID) is stored on-chain;
//! the actual bytes are held by *keeper* nodes and optionally pinned to IPFS /
//! Filecoin / Arweave.
//!
//! To ensure keepers are honest about what they hold, any peer may issue a
//! **Data Availability challenge**: the challenger picks a random nonce and a
//! byte range, broadcasts a [`DataAvailabilityChallenge`], and waits for a
//! [`DataAvailabilityChallengeResponse`] that proves the keeper holds the
//! bytes by returning `blake3(nonce ‖ fragment)`.
//!
//! # Design decisions
//!
//! * **blake3** — already canonical in the WRFCoin codebase; fast and DoS-hard.
//! * **Gossipsub broadcast** — challenges go out on the `wrfcoin/da-challenges/1`
//!   topic so any keeper subscribed to that topic can respond.
//! * **Fragment size cap** — responses are limited to [`CHALLENGE_FRAGMENT_MAX_BYTES`]
//!   to prevent amplification attacks.
//! * **Timeout** — keepers that do not respond within [`CHALLENGE_TIMEOUT_SECS`]
//!   seconds are eligible for slashing or reputation deduction (enforced by the
//!   application layer, not the P2P layer).

use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Constants ───────────────────────────────────────────────────────────────

/// Maximum byte length of the fragment a keeper must return in a challenge
/// response.  Keeping this at 4 KiB bounds the gossipsub message size.
pub const CHALLENGE_FRAGMENT_MAX_BYTES: u32 = 4_096;

/// Seconds after `issued_at` before a challenge expires.
/// Application layers should treat non-responses after this window as
/// evidence of unavailability.
pub const CHALLENGE_TIMEOUT_SECS: i64 = 30;

/// Maximum length of a CID string (enough for IPFS v1 base32 CIDs).
const MAX_CID_LEN: usize = 256;

// ── Error type ──────────────────────────────────────────────────────────────

/// Errors returned when validating a DA challenge or its response.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DaChallengeError {
    /// The `challenge_id` in the response does not match the challenge.
    #[error("challenge_id mismatch: expected '{expected}', got '{got}'")]
    ChallengeIdMismatch { expected: String, got: String },

    /// The returned data fragment does not match the expected bytes.
    #[error("data fragment mismatch: expected {} bytes, got {} bytes", expected_len, got_len)]
    FragmentMismatch { expected_len: usize, got_len: usize },

    /// The proof field does not equal `blake3(nonce ‖ fragment)`.
    #[error("proof verification failed")]
    ProofInvalid,

    /// The CID is empty or exceeds the maximum allowed length.
    #[error("invalid CID: {0}")]
    InvalidCid(String),

    /// The requested fragment length is zero or exceeds `CHALLENGE_FRAGMENT_MAX_BYTES`.
    #[error("invalid fragment length {0}: must be in 1..={}", CHALLENGE_FRAGMENT_MAX_BYTES)]
    InvalidFragmentLength(u32),
}

// ── Core types ──────────────────────────────────────────────────────────────

/// A Data Availability challenge broadcast over the P2P network.
///
/// The challenger asks the network: *"Can you prove you hold byte range
/// `[offset, offset + length)` of the blob identified by `cid`?"*
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataAvailabilityChallenge {
    /// Unique challenge identifier, derived from `blake3(nonce ‖ cid_bytes)`.
    pub challenge_id: String,
    /// Content identifier of the blob being challenged (e.g. `ipfs:QmXyz…`).
    pub cid: String,
    /// 256-bit random nonce chosen by the challenger.  The keeper must include
    /// this nonce when computing the proof so that responses cannot be
    /// pre-computed.
    pub nonce: [u8; 32],
    /// Byte offset into the blob from which the fragment must be read.
    pub offset: u64,
    /// Number of bytes to return.  Must be in `1..=CHALLENGE_FRAGMENT_MAX_BYTES`.
    pub length: u32,
    /// Unix timestamp (seconds) when the challenge was issued.
    pub issued_at: i64,
}

/// A keeper's response to a [`DataAvailabilityChallenge`].
///
/// The response proves the keeper holds the requested byte range by
/// returning the fragment together with `blake3(nonce ‖ fragment)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataAvailabilityChallengeResponse {
    /// Echo of [`DataAvailabilityChallenge::challenge_id`].
    pub challenge_id: String,
    /// The requested byte range from the blob.
    pub data_fragment: Vec<u8>,
    /// `blake3(nonce ‖ data_fragment)` — binds the fragment to the challenge
    /// nonce, preventing spoofed or pre-computed responses.
    pub proof: [u8; 32],
    /// Keeper's node identifier.
    pub keeper_id: String,
    /// Unix timestamp (seconds) when the response was generated.
    pub responded_at: i64,
}

// ── Helper functions ─────────────────────────────────────────────────────────

/// Compute `blake3(nonce ‖ fragment)`.
///
/// Both the challenger (after learning the fragment) and the verifier call
/// this function to check that the keeper's `proof` field is correct.
pub fn compute_proof(nonce: &[u8; 32], fragment: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(nonce);
    hasher.update(fragment);
    *hasher.finalize().as_bytes()
}

/// Generate a new [`DataAvailabilityChallenge`] with a cryptographically
/// random nonce for the given `cid` and byte range.
///
/// # Errors
///
/// Returns [`DaChallengeError::InvalidCid`] if `cid` is empty or too long,
/// or [`DaChallengeError::InvalidFragmentLength`] if `length` is out of
/// range.
pub fn generate_challenge(
    cid: &str,
    offset: u64,
    length: u32,
) -> Result<DataAvailabilityChallenge, DaChallengeError> {
    validate_cid(cid)?;
    validate_length(length)?;

    let mut nonce = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut nonce);

    let issued_at = chrono::Utc::now().timestamp();

    // challenge_id = hex(blake3(nonce ‖ cid_bytes))
    let challenge_id = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&nonce);
        hasher.update(cid.as_bytes());
        hex::encode(hasher.finalize().as_bytes())
    };

    Ok(DataAvailabilityChallenge {
        challenge_id,
        cid: cid.to_string(),
        nonce,
        offset,
        length,
        issued_at,
    })
}

/// Verify a keeper's challenge response against the original challenge and
/// the expected fragment bytes.
///
/// This function is called by the challenger (or any third-party verifier)
/// once they have retrieved the blob and can supply `expected_fragment`.
///
/// # Errors
///
/// * [`DaChallengeError::ChallengeIdMismatch`] — response has wrong ID.
/// * [`DaChallengeError::FragmentMismatch`] — response has wrong byte count.
/// * [`DaChallengeError::ProofInvalid`] — `blake3(nonce ‖ fragment)` does
///   not match the proof field.
pub fn verify_challenge_response(
    challenge: &DataAvailabilityChallenge,
    response: &DataAvailabilityChallengeResponse,
    expected_fragment: &[u8],
) -> Result<(), DaChallengeError> {
    // 1. challenge_id must echo the original challenge
    if response.challenge_id != challenge.challenge_id {
        return Err(DaChallengeError::ChallengeIdMismatch {
            expected: challenge.challenge_id.clone(),
            got: response.challenge_id.clone(),
        });
    }

    // 2. fragment must match expected bytes
    if response.data_fragment != expected_fragment {
        return Err(DaChallengeError::FragmentMismatch {
            expected_len: expected_fragment.len(),
            got_len: response.data_fragment.len(),
        });
    }

    // 3. proof must equal blake3(nonce ‖ fragment)
    let expected_proof = compute_proof(&challenge.nonce, &response.data_fragment);
    if response.proof != expected_proof {
        return Err(DaChallengeError::ProofInvalid);
    }

    Ok(())
}

// ── Internal validators ──────────────────────────────────────────────────────

fn validate_cid(cid: &str) -> Result<(), DaChallengeError> {
    if cid.is_empty() {
        return Err(DaChallengeError::InvalidCid("CID must not be empty".into()));
    }
    if cid.len() > MAX_CID_LEN {
        return Err(DaChallengeError::InvalidCid(format!(
            "CID length {} exceeds maximum {MAX_CID_LEN}",
            cid.len()
        )));
    }
    Ok(())
}

fn validate_length(length: u32) -> Result<(), DaChallengeError> {
    if length == 0 || length > CHALLENGE_FRAGMENT_MAX_BYTES {
        return Err(DaChallengeError::InvalidFragmentLength(length));
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CID: &str = "ipfs:QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG";

    // -- generate_challenge --------------------------------------------------

    #[test]
    fn generate_challenge_produces_unique_nonces() {
        let c1 = generate_challenge(TEST_CID, 0, 64).unwrap();
        let c2 = generate_challenge(TEST_CID, 0, 64).unwrap();
        assert_ne!(c1.nonce, c2.nonce, "consecutive challenges must have different nonces");
    }

    #[test]
    fn generate_challenge_produces_unique_ids() {
        let c1 = generate_challenge(TEST_CID, 0, 64).unwrap();
        let c2 = generate_challenge(TEST_CID, 0, 64).unwrap();
        assert_ne!(c1.challenge_id, c2.challenge_id);
    }

    #[test]
    fn generate_challenge_records_cid_and_range() {
        let c = generate_challenge(TEST_CID, 128, 512).unwrap();
        assert_eq!(c.cid, TEST_CID);
        assert_eq!(c.offset, 128);
        assert_eq!(c.length, 512);
    }

    #[test]
    fn generate_challenge_rejects_empty_cid() {
        let err = generate_challenge("", 0, 64).unwrap_err();
        assert!(matches!(err, DaChallengeError::InvalidCid(_)));
    }

    #[test]
    fn generate_challenge_rejects_cid_too_long() {
        let long_cid = "x".repeat(MAX_CID_LEN + 1);
        let err = generate_challenge(&long_cid, 0, 64).unwrap_err();
        assert!(matches!(err, DaChallengeError::InvalidCid(_)));
    }

    #[test]
    fn generate_challenge_rejects_zero_length() {
        let err = generate_challenge(TEST_CID, 0, 0).unwrap_err();
        assert!(matches!(err, DaChallengeError::InvalidFragmentLength(0)));
    }

    #[test]
    fn generate_challenge_rejects_oversized_length() {
        let err = generate_challenge(TEST_CID, 0, CHALLENGE_FRAGMENT_MAX_BYTES + 1).unwrap_err();
        assert!(matches!(
            err,
            DaChallengeError::InvalidFragmentLength(n) if n == CHALLENGE_FRAGMENT_MAX_BYTES + 1
        ));
    }

    #[test]
    fn generate_challenge_accepts_max_length() {
        generate_challenge(TEST_CID, 0, CHALLENGE_FRAGMENT_MAX_BYTES)
            .expect("max fragment length should be accepted");
    }

    // -- compute_proof -------------------------------------------------------

    #[test]
    fn compute_proof_is_deterministic() {
        let nonce = [42u8; 32];
        let fragment = b"hello world";
        let p1 = compute_proof(&nonce, fragment);
        let p2 = compute_proof(&nonce, fragment);
        assert_eq!(p1, p2);
    }

    #[test]
    fn compute_proof_differs_with_different_nonce() {
        let nonce1 = [1u8; 32];
        let nonce2 = [2u8; 32];
        let fragment = b"data";
        assert_ne!(compute_proof(&nonce1, fragment), compute_proof(&nonce2, fragment));
    }

    #[test]
    fn compute_proof_differs_with_different_fragment() {
        let nonce = [0u8; 32];
        assert_ne!(compute_proof(&nonce, b"a"), compute_proof(&nonce, b"b"));
    }

    // -- verify_challenge_response -------------------------------------------

    fn make_valid_response(
        challenge: &DataAvailabilityChallenge,
        fragment: &[u8],
    ) -> DataAvailabilityChallengeResponse {
        let proof = compute_proof(&challenge.nonce, fragment);
        DataAvailabilityChallengeResponse {
            challenge_id: challenge.challenge_id.clone(),
            data_fragment: fragment.to_vec(),
            proof,
            keeper_id: "keeper-001".to_string(),
            responded_at: chrono::Utc::now().timestamp(),
        }
    }

    #[test]
    fn verify_response_passes_on_correct_data() {
        let challenge = generate_challenge(TEST_CID, 0, 16).unwrap();
        let fragment = vec![0xAB_u8; 16];
        let response = make_valid_response(&challenge, &fragment);
        verify_challenge_response(&challenge, &response, &fragment).expect("valid response must pass");
    }

    #[test]
    fn verify_response_fails_on_challenge_id_mismatch() {
        let challenge = generate_challenge(TEST_CID, 0, 16).unwrap();
        let fragment = vec![0u8; 16];
        let mut response = make_valid_response(&challenge, &fragment);
        response.challenge_id = "wrong-id".to_string();
        let err = verify_challenge_response(&challenge, &response, &fragment).unwrap_err();
        assert!(matches!(err, DaChallengeError::ChallengeIdMismatch { .. }));
    }

    #[test]
    fn verify_response_fails_on_fragment_mismatch() {
        let challenge = generate_challenge(TEST_CID, 0, 16).unwrap();
        let fragment = vec![0u8; 16];
        let wrong_fragment = vec![1u8; 16];
        // response claims to return `fragment` but we compare against `wrong_fragment`
        let response = make_valid_response(&challenge, &fragment);
        let err = verify_challenge_response(&challenge, &response, &wrong_fragment).unwrap_err();
        assert!(matches!(err, DaChallengeError::FragmentMismatch { .. }));
    }

    #[test]
    fn verify_response_fails_on_bad_proof() {
        let challenge = generate_challenge(TEST_CID, 0, 16).unwrap();
        let fragment = vec![0u8; 16];
        let mut response = make_valid_response(&challenge, &fragment);
        response.proof = [0xFF_u8; 32]; // corrupt the proof
        let err = verify_challenge_response(&challenge, &response, &fragment).unwrap_err();
        assert!(matches!(err, DaChallengeError::ProofInvalid));
    }

    #[test]
    fn verify_response_fails_when_keeper_mutates_fragment_but_keeps_proof() {
        let challenge = generate_challenge(TEST_CID, 0, 4).unwrap();
        let original_fragment = vec![0xAA_u8; 4];
        let mut response = make_valid_response(&challenge, &original_fragment);
        // Keeper swaps in a different fragment without updating the proof
        response.data_fragment = vec![0xBB_u8; 4];
        let err = verify_challenge_response(&challenge, &response, &original_fragment).unwrap_err();
        // Fragment now differs from expected
        assert!(matches!(err, DaChallengeError::FragmentMismatch { .. }));
    }
}
