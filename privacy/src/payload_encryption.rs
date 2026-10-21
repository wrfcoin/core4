//! Transaction payload encryption using ChaCha20-Poly1305.
//!
//! Encrypts transaction metadata with an ECDH-derived shared secret so only
//! the intended recipient can read it.
//!
//! # Protocol
//!
//! 1. Sender generates an ephemeral Ristretto key pair `(r, R = r*G)`.
//! 2. Shared secret `S = r * P_recipient` (ECDH).
//! 3. Derive a 32-byte symmetric key from `S` via SHA3-256.
//! 4. Encrypt the payload with ChaCha20-Poly1305 using the derived key.
//! 5. Publish `(R, nonce, ciphertext, tag)` on-chain.
//!
//! The recipient computes `S' = p * R` (where `p` is their private key),
//! derives the same symmetric key, and decrypts.

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use curve25519_dalek::{
    constants::RISTRETTO_BASEPOINT_TABLE,
    ristretto::{CompressedRistretto, RistrettoPoint},
    scalar::Scalar,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

use crate::error::{PrivacyError, PrivacyResult};

/// An encrypted payload with all data needed for decryption by the recipient.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPayload {
    /// Sender's ephemeral public key `R = r*G` (32 bytes, compressed Ristretto).
    pub ephemeral_public_key: [u8; 32],
    /// 12-byte nonce for ChaCha20-Poly1305.
    pub nonce: [u8; 12],
    /// Encrypted data (ciphertext + 16-byte Poly1305 tag).
    #[serde(with = "hex_vec")]
    pub ciphertext: Vec<u8>,
}

/// Serde helper for hex-encoding `Vec<u8>`.
mod hex_vec {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Encrypt a payload for a recipient identified by their Ristretto public key.
///
/// Returns the encrypted payload containing the ephemeral public key, nonce,
/// and ciphertext.  The ephemeral private key is discarded after use.
///
/// # Arguments
///
/// - `plaintext` -- the data to encrypt.
/// - `recipient_public_key` -- 32-byte compressed Ristretto public key.
///
/// # Errors
///
/// Returns [`PrivacyError::Encryption`] if the public key is invalid or
/// encryption fails.
pub fn encrypt_payload(
    plaintext: &[u8],
    recipient_public_key: &[u8; 32],
) -> PrivacyResult<EncryptedPayload> {
    // Decompress recipient's public key.
    let recipient_point = decompress_ristretto(recipient_public_key)
        .ok_or_else(|| PrivacyError::Encryption("invalid recipient public key".into()))?;

    // Generate ephemeral key pair.
    let mut ephemeral_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut ephemeral_bytes);
    let ephemeral_scalar = Scalar::from_bytes_mod_order(ephemeral_bytes);
    let ephemeral_public = (&ephemeral_scalar * RISTRETTO_BASEPOINT_TABLE)
        .compress()
        .to_bytes();

    // ECDH shared secret: S = r * P_recipient.
    let shared_point = ephemeral_scalar * recipient_point;
    let symmetric_key = derive_key(&shared_point);

    // Generate random nonce.
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);

    // Encrypt with ChaCha20-Poly1305.
    let cipher = ChaCha20Poly1305::new_from_slice(&symmetric_key)
        .map_err(|e| PrivacyError::Encryption(format!("cipher init: {e}")))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| PrivacyError::Encryption(format!("encrypt: {e}")))?;

    Ok(EncryptedPayload {
        ephemeral_public_key: ephemeral_public,
        nonce: nonce_bytes,
        ciphertext,
    })
}

/// Decrypt a payload using the recipient's private key.
///
/// # Arguments
///
/// - `payload` -- the encrypted payload (ephemeral key + nonce + ciphertext).
/// - `private_key` -- the recipient's 32-byte Ristretto private key scalar.
///
/// # Errors
///
/// Returns [`PrivacyError::Encryption`] if the ephemeral key is invalid,
/// the key derivation fails, or the ciphertext is tampered with.
pub fn decrypt_payload(
    payload: &EncryptedPayload,
    private_key: &[u8; 32],
) -> PrivacyResult<Vec<u8>> {
    // Decompress the sender's ephemeral public key.
    let ephemeral_point = decompress_ristretto(&payload.ephemeral_public_key)
        .ok_or_else(|| PrivacyError::Encryption("invalid ephemeral public key".into()))?;

    // ECDH shared secret: S' = p * R.
    let recipient_scalar = Scalar::from_bytes_mod_order(*private_key);
    let shared_point = recipient_scalar * ephemeral_point;
    let symmetric_key = derive_key(&shared_point);

    // Decrypt with ChaCha20-Poly1305.
    let cipher = ChaCha20Poly1305::new_from_slice(&symmetric_key)
        .map_err(|e| PrivacyError::Encryption(format!("cipher init: {e}")))?;
    let nonce = Nonce::from_slice(&payload.nonce);
    let plaintext = cipher
        .decrypt(nonce, payload.ciphertext.as_ref())
        .map_err(|_| PrivacyError::Encryption("decryption failed (invalid key or tampered data)".into()))?;

    Ok(plaintext)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive a 32-byte symmetric key from a shared ECDH point.
fn derive_key(shared_point: &RistrettoPoint) -> [u8; 32] {
    let mut hasher = Sha3_256::new();
    hasher.update(b"wrfcoin-payload-key-v1");
    hasher.update(shared_point.compress().as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

/// Decompress a 32-byte compressed Ristretto point.
fn decompress_ristretto(bytes: &[u8; 32]) -> Option<RistrettoPoint> {
    CompressedRistretto::from_slice(bytes)
        .ok()
        .and_then(|c| c.decompress())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_keypair() -> ([u8; 32], [u8; 32]) {
        let mut priv_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut priv_bytes);
        let scalar = Scalar::from_bytes_mod_order(priv_bytes);
        let public = (&scalar * RISTRETTO_BASEPOINT_TABLE)
            .compress()
            .to_bytes();
        (scalar.to_bytes(), public)
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let (priv_key, pub_key) = generate_keypair();
        let plaintext = b"confidential transaction data";

        let encrypted = encrypt_payload(plaintext, &pub_key).unwrap();
        let decrypted = decrypt_payload(&encrypted, &priv_key).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let (_, pub_key) = generate_keypair();
        let (wrong_priv, _) = generate_keypair();
        let plaintext = b"secret data";

        let encrypted = encrypt_payload(plaintext, &pub_key).unwrap();
        let result = decrypt_payload(&encrypted, &wrong_priv);

        assert!(result.is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let (priv_key, pub_key) = generate_keypair();
        let plaintext = b"important data";

        let mut encrypted = encrypt_payload(plaintext, &pub_key).unwrap();
        // Flip a byte in the ciphertext.
        if let Some(byte) = encrypted.ciphertext.first_mut() {
            *byte ^= 0xFF;
        }

        let result = decrypt_payload(&encrypted, &priv_key);
        assert!(result.is_err());
    }

    #[test]
    fn empty_plaintext() {
        let (priv_key, pub_key) = generate_keypair();
        let plaintext = b"";

        let encrypted = encrypt_payload(plaintext, &pub_key).unwrap();
        let decrypted = decrypt_payload(&encrypted, &priv_key).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn large_plaintext() {
        let (priv_key, pub_key) = generate_keypair();
        let plaintext = vec![0xABu8; 10_000];

        let encrypted = encrypt_payload(&plaintext, &pub_key).unwrap();
        let decrypted = decrypt_payload(&encrypted, &priv_key).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn invalid_recipient_key_rejected() {
        // Use bytes that do not represent a valid compressed Ristretto point.
        // All-zero is the identity (valid), so use 0xFF-filled bytes instead.
        let result = encrypt_payload(b"test", &[0xFFu8; 32]);
        assert!(result.is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let (_, pub_key) = generate_keypair();
        let plaintext = b"serialization test";

        let encrypted = encrypt_payload(plaintext, &pub_key).unwrap();
        let json = serde_json::to_string(&encrypted).unwrap();
        let recovered: EncryptedPayload = serde_json::from_str(&json).unwrap();

        assert_eq!(encrypted.ephemeral_public_key, recovered.ephemeral_public_key);
        assert_eq!(encrypted.nonce, recovered.nonce);
        assert_eq!(encrypted.ciphertext, recovered.ciphertext);
    }

    #[test]
    fn each_encryption_uses_unique_ephemeral_key() {
        let (_, pub_key) = generate_keypair();
        let plaintext = b"test";

        let e1 = encrypt_payload(plaintext, &pub_key).unwrap();
        let e2 = encrypt_payload(plaintext, &pub_key).unwrap();

        assert_ne!(e1.ephemeral_public_key, e2.ephemeral_public_key);
        assert_ne!(e1.ciphertext, e2.ciphertext);
    }
}
