#![forbid(unsafe_code)]
//! WRFCoin privacy-enhancing cryptographic primitives.
//!
//! This crate provides standalone, production-grade privacy technologies:
//!
//! - **Pedersen commitments** -- hide transaction amounts while preserving
//!   additive homomorphism for balance verification.
//! - **Range proofs** -- prove a committed value lies in `[0, 2^n)` without
//!   revealing it.
//! - **Ring signatures (MLSAG)** -- hide the sender among a ring of decoys.
//! - **Stealth addresses** -- one-time addresses for receiver privacy.
//! - **Payload encryption** -- ChaCha20-Poly1305 AEAD for transaction data.
//! - **Key-image registry** -- double-spend prevention for ring signatures.
//! - **P-256 ECDSA** -- secp256r1 verification for WebAuthn passkeys (#2169).

pub mod error;
pub mod key_image_registry;
pub mod mlsag_budget;
pub mod p256_ecdsa;
pub mod payload_encryption;
pub mod pedersen;
pub mod range_proof;
pub mod ring_signatures;
pub mod stealth_addresses;
pub mod types;

#[cfg(test)]
mod tests;

// ---- Convenience re-exports ------------------------------------------------

pub use error::{PrivacyError, PrivacyResult};
pub use key_image_registry::KeyImageRegistry;
pub use p256_ecdsa::{P256Error, P256PublicKey, P256Signature, P256SigningKey};
pub use mlsag_budget::{
    verify_batch_budgeted, verify_deferred, MlsagBudgetResult, MlsagVerifyItem,
    MLSAG_VERIFY_BUDGET_MS_DEFAULT,
};
pub use payload_encryption::{decrypt_payload, encrypt_payload, EncryptedPayload};
pub use pedersen::{ConfidentialAsset, PedersenCommitment};
pub use range_proof::{AggregatedRangeProof, RangeProof};
pub use ring_signatures::{KeyImage, MLSAGSignature, RingMember, RingSignatureParams};
pub use stealth_addresses::{StealthAddress, StealthKeys};
pub use types::{PrivacyLevel, PrivacySettings};

/// Re-export `Scalar` so downstream crates can construct blinding factors
/// without depending directly on `curve25519-dalek`.
pub use curve25519_dalek::scalar::Scalar;

/// Derive the 32-byte compressed Ristretto public key from a scalar private key.
///
/// Computes `P = x * G` and returns the 32-byte compressed representation.
pub fn scalar_to_public_key(scalar: &Scalar) -> [u8; 32] {
    use curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;
    (scalar * RISTRETTO_BASEPOINT_TABLE).compress().to_bytes()
}

#[cfg(test)]
mod integration_target_registration {
    /// #2888: a new `tests/*.rs` target must be registered in
    /// `scripts/integration_test_ratchet.json` or nothing in CI ever runs it.
    /// This fails at `cargo test -p wrfcoin-privacy`, i.e. at the point of
    /// addition, not in a later PR's ratchet.
    #[test]
    fn every_integration_target_under_tests_is_in_the_ratchet_inventory() {
        wrf_primitives::integration_inventory::assert_registered(
            env!("CARGO_MANIFEST_DIR"),
            env!("CARGO_PKG_NAME"),
        );
    }
}
