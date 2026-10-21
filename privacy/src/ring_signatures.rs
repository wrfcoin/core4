//! MLSAG ring signatures for sender anonymity.
//!
//! Ring signatures allow a signer to produce a signature that is verifiable
//! as coming from *one of* a set of public keys, without revealing *which* one.
//!
//! This module implements MLSAG (Multilayered Linkable Spontaneous Anonymous
//! Group) signatures using Ristretto points on Curve25519, with SHA3-256 for
//! all hash operations.
//!
//! # Key image
//!
//! Each private key maps to a unique **key image** `I = x * Hp(P)` where `Hp`
//! is a hash-to-point function.  The key image is published with every ring
//! signature so that double-spends can be detected without revealing the
//! signer.
//!
//! # Security considerations
//!
//! - The minimum ring size should be at least 7 for meaningful anonymity.
//! - All randomness is sourced from the operating system CSPRNG via [`OsRng`].
//! - SHA3-256 is used throughout for domain-separated hashing.
//! - Key material is handled as fixed-size byte arrays to avoid heap copies.

/// Minimum ring size (total members including signer) for meaningful anonymity.
pub const MIN_RING_SIZE: usize = 7;

/// Maximum ring size enforced during verification.
///
/// Each ring member requires ≈8 EC scalar-multiplication operations.  Without
/// this bound an attacker can craft a peer-supplied signature (or block-level
/// `RewardClaim`) with an enormous ring and force O(n) CPU work on every
/// verifying node.  1024 members provides ample privacy while keeping worst-case
/// per-signature verification time < 100 ms on constrained hardware (N100 CPU).
pub const MAX_RING_SIZE: usize = 1024;

use curve25519_dalek::{
    constants::RISTRETTO_BASEPOINT_TABLE,
    ristretto::{CompressedRistretto, RistrettoPoint},
    scalar::Scalar,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256, Sha3_512};

use crate::error::{PrivacyError, PrivacyResult};

// ---------------------------------------------------------------------------
// Entropy source
// ---------------------------------------------------------------------------

/// Trait abstracting a cryptographic entropy source.
///
/// The default implementation wraps [`OsRng`] from `rand_core`.  This can be
/// replaced in tests with a deterministic source.
pub trait EntropySource: Send + Sync {
    /// Fill `dest` with cryptographically secure random bytes.
    fn fill_bytes(&self, dest: &mut [u8]);
}

/// Default OS-backed entropy source.
#[derive(Debug, Clone, Copy)]
pub struct SystemEntropy;

impl EntropySource for SystemEntropy {
    fn fill_bytes(&self, dest: &mut [u8]) {
        OsRng.fill_bytes(dest);
    }
}

// ---------------------------------------------------------------------------
// Hash-to-point helper
// ---------------------------------------------------------------------------

/// Domain-separated hash-to-point using SHA3-512 and Ristretto `from_uniform_bytes`.
///
/// Computes `Hp(data)` by hashing `"wrfcoin-hash-to-point-v1" || data` with
/// SHA3-512 to produce 64 uniform bytes, then mapping to a Ristretto point.
fn hash_to_point(data: &[u8]) -> RistrettoPoint {
    let mut hasher = Sha3_512::new();
    hasher.update(b"wrfcoin-hash-to-point-v1");
    hasher.update(data);
    let hash_output = hasher.finalize();
    // from_uniform_bytes requires exactly 64 bytes
    let mut uniform = [0u8; 64];
    uniform.copy_from_slice(&hash_output);
    RistrettoPoint::from_uniform_bytes(&uniform)
}


// ---------------------------------------------------------------------------
// KeyImage
// ---------------------------------------------------------------------------

/// A key image uniquely derived from a private key.
///
/// `I = x * Hp(P)` where `x` is the private key scalar and `P = x * G` is the
/// corresponding public key.  Because `Hp` is a one-way hash-to-point, the
/// key image is deterministic for a given key pair but reveals nothing about
/// the private key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyImage {
    /// The 32-byte compressed Ristretto representation of the key image point.
    pub image: [u8; 32],
}

impl PartialEq for KeyImage {
    fn eq(&self, other: &Self) -> bool {
        self.image == other.image
    }
}

impl Eq for KeyImage {}

impl std::hash::Hash for KeyImage {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.image.hash(state);
    }
}

impl KeyImage {
    /// Generate a key image from a 32-byte private key.
    ///
    /// Computes `I = x * Hp(P)` where `P = x * G`.
    ///
    /// # Errors
    ///
    /// Returns [`PrivacyError::InvalidKey`] if the resulting point is the
    /// identity (all-zero image), which would indicate a degenerate key.
    pub fn generate(private_key: &[u8; 32]) -> PrivacyResult<Self> {
        let scalar = Scalar::from_bytes_mod_order(*private_key);

        // Compute the public key P = x * G
        let public_point = &scalar * RISTRETTO_BASEPOINT_TABLE;
        let public_compressed = public_point.compress();

        // Hash-to-point: Hp(P)
        let hp = hash_to_point(public_compressed.as_bytes());

        // Key image: I = x * Hp(P)
        let image_point = scalar * hp;
        let image_bytes = image_point.compress().to_bytes();

        // Reject degenerate (identity) key images
        if image_bytes.iter().all(|&b| b == 0) {
            return Err(PrivacyError::InvalidKey(
                "key image is the identity point".into(),
            ));
        }

        Ok(Self {
            image: image_bytes,
        })
    }

    /// Return the 32-byte representation.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.image
    }

    /// Construct a `KeyImage` from raw bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self { image: *bytes }
    }
}

// ---------------------------------------------------------------------------
// RingMember
// ---------------------------------------------------------------------------

/// A member of a ring, identified by their compressed Ristretto public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RingMember {
    /// 32-byte compressed Ristretto public key.
    pub public_key: [u8; 32],
}

impl RingMember {
    /// Decompress the stored public key into a Ristretto point.
    ///
    /// # Errors
    ///
    /// Returns [`PrivacyError::InvalidKey`] if the bytes do not represent a
    /// valid compressed Ristretto point.
    fn to_point(&self) -> PrivacyResult<RistrettoPoint> {
        CompressedRistretto::from_slice(&self.public_key)
            .map_err(|_| PrivacyError::InvalidKey("invalid compressed Ristretto point".into()))?
            .decompress()
            .ok_or_else(|| PrivacyError::InvalidKey("point decompression failed".into()))
    }
}

// ---------------------------------------------------------------------------
// RingSignatureParams
// ---------------------------------------------------------------------------

/// Parameters controlling ring signature creation.
#[derive(Debug, Clone)]
pub struct RingSignatureParams {
    /// Number of members in the ring (including the real signer).
    pub ring_size: usize,
    /// The message being signed.
    pub message: Vec<u8>,
}

// ---------------------------------------------------------------------------
// MLSAGSignature
// ---------------------------------------------------------------------------

/// An MLSAG (Multilayered Linkable Spontaneous Anonymous Group) ring signature.
///
/// The signature proves that the signer possesses the private key for one of
/// the public keys in `ring`, without revealing which one.  The embedded
/// `key_image` provides linkability for double-spend detection.
///
/// # Serialization
///
/// The `c` and `s` fields are serialized as hex strings for portability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLSAGSignature {
    /// Key image linking this signature to the signer's private key.
    pub key_image: KeyImage,

    /// Initial challenge `c_0` as a 32-byte scalar (little-endian).
    #[serde(with = "hex_bytes")]
    pub c: Vec<u8>,

    /// Response scalars `s_0 .. s_{n-1}`, each 32 bytes (little-endian).
    ///
    /// `s[i]` is a Vec<u8> of length 32.
    pub s: Vec<Vec<u8>>,

    /// The ring of public keys used in this signature.
    pub ring: Vec<RingMember>,
}

/// Serde helper for hex-encoding `Vec<u8>`.
mod hex_bytes {
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

impl MLSAGSignature {
    /// Produce an MLSAG ring signature.
    ///
    /// # Arguments
    ///
    /// - `message` -- the data being signed.
    /// - `private_key` -- the signer's 32-byte private key scalar.
    /// - `ring_members` -- the *decoy* public keys.  The signer's public key
    ///   is derived from `private_key` and inserted into the ring at a random
    ///   position.
    ///
    /// # Algorithm
    ///
    /// 1. Compute the signer's public key `P_j = x * G` and key image
    ///    `I = x * Hp(P_j)`.
    /// 2. Insert the signer at a random position `j` in the ring.
    /// 3. Generate random nonce `alpha` for position `j`.
    /// 4. Compute `L_j = alpha * G`, `R_j = alpha * Hp(P_j)`.
    /// 5. For each subsequent position `i = j+1 .. j-1 (mod n)`:
    ///    - Sample random `s_i`.
    ///    - Compute `c_{i+1} = H(m, L_i, R_i)`.
    ///    - Compute `L_i = s_i * G + c_i * P_i`,
    ///      `R_i = s_i * Hp(P_i) + c_i * I`.
    /// 6. Close the ring: `s_j = alpha - c_j * x`.
    ///
    /// # Errors
    ///
    /// Returns [`PrivacyError::RingSignature`] if:
    /// - `ring_members` is empty.
    /// - Any ring member has an invalid public key.
    pub fn sign(
        message: &[u8],
        private_key: &[u8; 32],
        ring_members: &[RingMember],
    ) -> PrivacyResult<Self> {
        if ring_members.is_empty() {
            return Err(PrivacyError::RingSignature(
                "ring must contain at least one decoy member".into(),
            ));
        }
        if ring_members.len() + 1 < MIN_RING_SIZE {
            return Err(PrivacyError::RingSignature(format!(
                "ring size {} is below minimum {MIN_RING_SIZE}",
                ring_members.len() + 1
            )));
        }

        let x = Scalar::from_bytes_mod_order(*private_key);
        let signer_public = &x * RISTRETTO_BASEPOINT_TABLE;
        let signer_compressed = signer_public.compress().to_bytes();

        // Build the full ring with the signer inserted at a random position.
        let total_size = ring_members.len() + 1;
        let signer_index = random_index(total_size);

        let mut ring: Vec<RingMember> = Vec::with_capacity(total_size);
        let mut decoy_iter = ring_members.iter();
        for i in 0..total_size {
            if i == signer_index {
                ring.push(RingMember {
                    public_key: signer_compressed,
                });
            } else {
                let member = decoy_iter.next().ok_or_else(|| {
                    PrivacyError::RingSignature("ring member iterator exhausted".into())
                })?;
                ring.push(member.clone());
            }
        }

        // Decompress all ring public keys and compute Hp(P_i) for each.
        let mut ring_points: Vec<RistrettoPoint> = Vec::with_capacity(total_size);
        let mut ring_hp: Vec<RistrettoPoint> = Vec::with_capacity(total_size);
        for member in &ring {
            let point = member.to_point()?;
            let hp = hash_to_point(&member.public_key);
            ring_points.push(point);
            ring_hp.push(hp);
        }

        // Key image: I = x * Hp(P_j)
        let key_image_point = x * ring_hp[signer_index];
        let key_image = KeyImage {
            image: key_image_point.compress().to_bytes(),
        };

        // Generate random nonce alpha for the signer position.
        let alpha = random_scalar();

        // L_j = alpha * G
        let l_j = &alpha * RISTRETTO_BASEPOINT_TABLE;
        // R_j = alpha * Hp(P_j)
        let r_j = alpha * ring_hp[signer_index];

        // Allocate storage for challenges and responses.
        let mut c_values: Vec<Scalar> = vec![Scalar::ZERO; total_size];
        let mut s_values: Vec<Scalar> = vec![Scalar::ZERO; total_size];

        // Compute c_{j+1} = H(m, L_j, R_j, I)
        let next_index = (signer_index + 1) % total_size;
        c_values[next_index] = challenge_hash(message, &l_j, &r_j, &key_image);

        // Canonical MLSAG ring walk (#816): visit every non-signer position
        // in order j+1, j+2, ..., j-1 (mod n).
        for step in 1..total_size {
            let i = (signer_index + step) % total_size;
            s_values[i] = random_scalar();
            // L_i = s_i * G + c_i * P_i
            let l_i =
                &s_values[i] * RISTRETTO_BASEPOINT_TABLE + c_values[i] * ring_points[i];
            // R_i = s_i * Hp(P_i) + c_i * I
            let r_i = s_values[i] * ring_hp[i] + c_values[i] * key_image_point;
            // c_{i+1} = H(m, L_i, R_i, I)
            c_values[(i + 1) % total_size] = challenge_hash(message, &l_i, &r_i, &key_image);
        }

        // Close the ring for the signer: s_j = alpha - c_j * x
        s_values[signer_index] = alpha - c_values[signer_index] * x;

        // Self-consistency check (#816): recompute the signer's challenge from
        // (L_j, R_j) using the closed s_j and verify it matches c_{j+1}.
        let l_check = &s_values[signer_index] * RISTRETTO_BASEPOINT_TABLE
            + c_values[signer_index] * ring_points[signer_index];
        let r_check = s_values[signer_index] * ring_hp[signer_index]
            + c_values[signer_index] * key_image_point;
        let c_check = challenge_hash(message, &l_check, &r_check, &key_image);
        if c_check != c_values[next_index] {
            return Err(PrivacyError::RingSignature(
                "ring closure consistency check failed — internal signing error".into(),
            ));
        }

        // Package the signature.  We store c_0 as the starting challenge.
        let c0_bytes = c_values[0].to_bytes().to_vec();
        let s_bytes: Vec<Vec<u8>> = s_values.iter().map(|s| s.to_bytes().to_vec()).collect();

        Ok(Self {
            key_image,
            c: c0_bytes,
            s: s_bytes,
            ring,
        })
    }

    /// Verify this MLSAG ring signature against `message`.
    ///
    /// Returns `Ok(true)` if the signature is valid, `Ok(false)` otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`PrivacyError::RingSignature`] if the signature structure is
    /// malformed (wrong number of responses, invalid points, etc.).
    pub fn verify(&self, message: &[u8]) -> PrivacyResult<bool> {
        let n = self.ring.len();
        if n == 0 {
            return Err(PrivacyError::RingSignature("empty ring".into()));
        }
        // Guard against CPU DoS via attacker-controlled ring sizes.  A peer-
        // supplied block containing a `RewardClaim` with a huge ring would
        // otherwise force O(n) EC operations on every verifying node.
        if n > MAX_RING_SIZE {
            return Err(PrivacyError::RingSignature(format!(
                "ring size {n} exceeds maximum {MAX_RING_SIZE}"
            )));
        }
        if self.s.len() != n {
            return Err(PrivacyError::RingSignature(
                "response count does not match ring size".into(),
            ));
        }
        if self.c.len() != 32 {
            return Err(PrivacyError::RingSignature(
                "initial challenge must be 32 bytes".into(),
            ));
        }

        // Reconstruct key image point.
        let ki_compressed = CompressedRistretto::from_slice(&self.key_image.image)
            .map_err(|_| PrivacyError::RingSignature("invalid key image encoding".into()))?;
        let key_image_point = ki_compressed
            .decompress()
            .ok_or_else(|| PrivacyError::RingSignature("key image decompression failed".into()))?;

        // Parse c_0.
        let c0_bytes: [u8; 32] = self.c[..32]
            .try_into()
            .map_err(|_| PrivacyError::RingSignature("bad challenge length".into()))?;
        let mut current_c = Scalar::from_bytes_mod_order(c0_bytes);

        // Walk the ring, recomputing each challenge.
        for i in 0..n {
            let point_i = self.ring[i].to_point()?;
            let hp_i = hash_to_point(&self.ring[i].public_key);

            // Use as_slice().try_into() instead of [..32].try_into() so that
            // a too-short scalar (e.g. from a crafted peer-supplied signature)
            // returns Err instead of panicking with an index-out-of-bounds.
            // This also rejects scalars longer than 32 bytes (malleability).
            let s_bytes: [u8; 32] = self.s[i]
                .as_slice()
                .try_into()
                .map_err(|_| PrivacyError::RingSignature(format!(
                    "response scalar {i} has wrong length: {} (expected 32)",
                    self.s[i].len()
                )))?;
            let s_i = Scalar::from_bytes_mod_order(s_bytes);

            // L_i = s_i * G + c_i * P_i
            let l_i = &s_i * RISTRETTO_BASEPOINT_TABLE + current_c * point_i;
            // R_i = s_i * Hp(P_i) + c_i * I
            let r_i = s_i * hp_i + current_c * key_image_point;

            // c_{i+1} = H(m, L_i, R_i, I)
            current_c = challenge_hash(message, &l_i, &r_i, &key_image_point);
        }

        // The ring is valid iff we arrive back at c_0.
        let c0_original = Scalar::from_bytes_mod_order(c0_bytes);
        Ok(current_c == c0_original)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Compute a Fiat-Shamir challenge: `H("wrfcoin-mlsag-v1" || m || L || R || I) -> Scalar`.
fn challenge_hash(
    message: &[u8],
    l: &RistrettoPoint,
    r: &RistrettoPoint,
    key_image: &impl CompressPoint,
) -> Scalar {
    let mut hasher = Sha3_256::new();
    hasher.update(b"wrfcoin-mlsag-v1");
    hasher.update(message);
    hasher.update(l.compress().as_bytes());
    hasher.update(r.compress().as_bytes());
    hasher.update(key_image.compress_to_bytes());
    let hash = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&hash);
    Scalar::from_bytes_mod_order(bytes)
}

/// Helper trait to allow both `RistrettoPoint` and `KeyImage` in challenge hash.
trait CompressPoint {
    fn compress_to_bytes(&self) -> Vec<u8>;
}

impl CompressPoint for RistrettoPoint {
    fn compress_to_bytes(&self) -> Vec<u8> {
        self.compress().as_bytes().to_vec()
    }
}

impl CompressPoint for KeyImage {
    fn compress_to_bytes(&self) -> Vec<u8> {
        self.image.to_vec()
    }
}

/// Generate a random `Scalar` from OS entropy.
fn random_scalar() -> Scalar {
    let mut bytes = [0u8; 64];
    OsRng.fill_bytes(&mut bytes);
    Scalar::from_bytes_mod_order_wide(&bytes)
}

/// Generate a random index in `[0, upper)` using rejection sampling to
/// eliminate modulo bias.
fn random_index(upper: usize) -> usize {
    assert!(upper > 0, "random_index: upper bound must be > 0");
    let upper_u64 = upper as u64;
    let limit = u64::MAX - (u64::MAX % upper_u64);
    loop {
        let mut buf = [0u8; 8];
        OsRng.fill_bytes(&mut buf);
        let val = u64::from_le_bytes(buf);
        if val < limit {
            return (val % upper_u64) as usize;
        }
        // Reject and retry — probability of retry is < 2^{-56} for any
        // ring size ≤ 256, so this loop terminates instantly in practice.
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a key pair: (private scalar bytes, compressed public key bytes).
    fn generate_keypair() -> ([u8; 32], [u8; 32]) {
        let mut priv_bytes = [0u8; 64];
        OsRng.fill_bytes(&mut priv_bytes);
        let scalar = Scalar::from_bytes_mod_order_wide(&priv_bytes);
        let public = (&scalar * RISTRETTO_BASEPOINT_TABLE).compress().to_bytes();
        (scalar.to_bytes(), public)
    }

    #[test]
    fn key_image_deterministic() -> PrivacyResult<()> {
        let (priv_key, _) = generate_keypair();
        let ki1 = KeyImage::generate(&priv_key)?;
        let ki2 = KeyImage::generate(&priv_key)?;
        assert_eq!(ki1, ki2, "same private key must produce the same key image");
        Ok(())
    }

    #[test]
    fn key_image_different_keys_differ() -> PrivacyResult<()> {
        let (pk1, _) = generate_keypair();
        let (pk2, _) = generate_keypair();
        let ki1 = KeyImage::generate(&pk1)?;
        let ki2 = KeyImage::generate(&pk2)?;
        assert_ne!(ki1, ki2, "different keys must produce different key images");
        Ok(())
    }

    #[test]
    fn key_image_roundtrip() -> PrivacyResult<()> {
        let (priv_key, _) = generate_keypair();
        let ki = KeyImage::generate(&priv_key)?;
        let bytes = ki.to_bytes();
        let ki2 = KeyImage::from_bytes(&bytes);
        assert_eq!(ki, ki2);
        Ok(())
    }

    #[test]
    fn sign_and_verify_small_ring() -> PrivacyResult<()> {
        let (signer_priv, _signer_pub) = generate_keypair();
        // MIN_RING_SIZE - 1 decoys so total ring size meets the minimum.
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let message = b"test transaction payload";
        let sig = MLSAGSignature::sign(message, &signer_priv, &decoys)?;
        assert!(sig.verify(message)?, "signature must verify");
        Ok(())
    }

    #[test]
    fn sign_and_verify_large_ring() -> PrivacyResult<()> {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..15)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let message = b"large ring transaction";
        let sig = MLSAGSignature::sign(message, &signer_priv, &decoys)?;
        assert!(sig.verify(message)?);
        Ok(())
    }

    #[test]
    fn verify_rejects_wrong_message() -> PrivacyResult<()> {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let sig = MLSAGSignature::sign(b"original", &signer_priv, &decoys)?;
        assert!(
            !sig.verify(b"tampered")?,
            "signature must not verify against a different message"
        );
        Ok(())
    }

    #[test]
    fn verify_rejects_tampered_response() -> PrivacyResult<()> {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let mut sig = MLSAGSignature::sign(b"msg", &signer_priv, &decoys)?;
        // Tamper with one response scalar.
        sig.s[0] = vec![0xffu8; 32];
        assert!(
            !sig.verify(b"msg")?,
            "tampered signature must not verify"
        );
        Ok(())
    }

    #[test]
    fn sign_empty_decoys_fails() {
        let (signer_priv, _) = generate_keypair();
        let result = MLSAGSignature::sign(b"msg", &signer_priv, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn key_image_serialization_roundtrip() -> PrivacyResult<()> {
        let (priv_key, _) = generate_keypair();
        let ki = KeyImage::generate(&priv_key)?;
        let json = serde_json::to_string(&ki)?;
        let ki2: KeyImage = serde_json::from_str(&json)?;
        assert_eq!(ki, ki2);
        Ok(())
    }

    #[test]
    fn signature_serialization_roundtrip() -> PrivacyResult<()> {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let sig = MLSAGSignature::sign(b"roundtrip test", &signer_priv, &decoys)?;
        let json = serde_json::to_string(&sig)?;
        let sig2: MLSAGSignature = serde_json::from_str(&json)?;
        assert!(sig2.verify(b"roundtrip test")?);
        Ok(())
    }

    #[test]
    fn key_image_in_signature_matches_standalone() -> PrivacyResult<()> {
        let (signer_priv, _) = generate_keypair();
        let standalone_ki = KeyImage::generate(&signer_priv)?;

        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let sig = MLSAGSignature::sign(b"ki check", &signer_priv, &decoys)?;
        assert_eq!(
            sig.key_image, standalone_ki,
            "key image in signature must match standalone generation"
        );
        Ok(())
    }

    #[test]
    fn key_image_with_zero_private_key_returns_err() {
        // A zero scalar yields identity * Hp(P) = identity point (all-zero image bytes).
        // KeyImage::generate must return Err, not panic.
        let zero_key = [0u8; 32];
        let result = KeyImage::generate(&zero_key);
        assert!(result.is_err(), "expected Err for degenerate (zero) private key");
    }

    #[test]
    fn sign_with_invalid_ring_member_returns_err() {
        let (priv_key, _) = generate_keypair();
        // 0xFF-filled bytes are not a valid compressed Ristretto point encoding.
        let bad_member = RingMember { public_key: [0xFFu8; 32] };
        let mut decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();
        decoys[0] = bad_member;
        let result = MLSAGSignature::sign(b"test message", &priv_key, &decoys);
        assert!(result.is_err(), "expected Err for invalid ring member");
    }

    /// Regression test for #816: verify ring closure at every possible signer
    /// position.  The old loop had dead code for the closing challenge; the
    /// canonical construction must produce valid signatures regardless of where
    /// the signer lands in the ring.
    #[test]
    fn ring_closure_all_signer_positions() -> PrivacyResult<()> {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let message = b"ring closure regression #816";

        // Sign many times — the signer position is randomized each time.
        // With MIN_RING_SIZE = 7, after ~50 iterations every position is
        // very likely to have been hit at least once.
        for _ in 0..50 {
            let sig = MLSAGSignature::sign(message, &signer_priv, &decoys)?;
            assert!(
                sig.verify(message)?,
                "signature must verify at every signer position"
            );
        }
        Ok(())
    }

    /// Verify that the self-consistency check in sign() catches a
    /// hypothetical internal error (simulated by ensuring the check runs
    /// without panicking on normal inputs).
    #[test]
    fn ring_closure_self_check_passes() -> PrivacyResult<()> {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        // If the self-check fails, sign() returns Err. This must always succeed.
        let sig = MLSAGSignature::sign(b"self-check test", &signer_priv, &decoys)?;
        assert!(sig.verify(b"self-check test")?);
        Ok(())
    }

    #[test]
    fn random_index_covers_all_indices() {
        let upper = 7;
        let mut seen = vec![false; upper];
        for _ in 0..10_000 {
            let idx = random_index(upper);
            assert!(idx < upper);
            seen[idx] = true;
        }
        for (i, &was_seen) in seen.iter().enumerate() {
            assert!(was_seen, "index {i} was never produced in 10 000 samples");
        }
    }

    #[test]
    fn random_index_single_element() {
        for _ in 0..100 {
            assert_eq!(random_index(1), 0);
        }
    }

    #[test]
    fn random_index_two_elements() {
        let mut seen_zero = false;
        let mut seen_one = false;
        for _ in 0..100 {
            match random_index(2) {
                0 => seen_zero = true,
                1 => seen_one = true,
                other => panic!("random_index(2) returned {other}"),
            }
        }
        assert!(seen_zero, "index 0 was never produced");
        assert!(seen_one, "index 1 was never produced");
    }

    // ── #1735 hardening batch 9a: verify() structural validation gates ──────────
    //
    // PR #1990 added MAX_RING_SIZE and the as_slice().try_into() fix.  The
    // following tests pin the *other* structural guards in verify() that are
    // not yet covered: empty ring (line 410), response-count mismatch (line 421),
    // and short challenge scalar (line 426).  All three are consensus-path
    // crash/hang vectors if silently removed.

    /// `verify()` on a signature with an empty ring must return `Err`, not panic
    /// or loop forever.  The guard is `if n == 0 { return Err(...) }` at line 410.
    #[test]
    fn verify_rejects_empty_ring() {
        // Construct a syntactically-valid MLSAGSignature with an empty ring.
        // We use a valid-looking key image (non-identity) and a well-formed 32-byte
        // challenge so the *only* failure path is the empty-ring guard.
        let sig = MLSAGSignature {
            key_image: KeyImage::from_bytes(&[1u8; 32]),
            c: vec![0u8; 32],
            s: vec![],
            ring: vec![],
        };
        let result = sig.verify(b"msg");
        assert!(
            result.is_err(),
            "verify() on an empty ring must return Err, not Ok or panic"
        );
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("empty ring"),
            "error must mention 'empty ring', got: {err_msg}"
        );
    }

    /// `verify()` must return `Err` when `s.len() != ring.len()`.
    ///
    /// The guard is `if self.s.len() != n { return Err(...) }` at line 421.
    /// Without it, the loop would either index out of bounds or silently verify
    /// a structurally corrupt signature.
    #[test]
    fn verify_rejects_response_count_mismatch() {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let mut sig = MLSAGSignature::sign(b"msg", &signer_priv, &decoys)
            .expect("sign must succeed");

        // Append one extra response scalar so s.len() == ring.len() + 1.
        sig.s.push(vec![0xaau8; 32]);

        let result = sig.verify(b"msg");
        assert!(
            result.is_err(),
            "verify() must return Err when s.len() != ring.len(), got: {result:?}"
        );
    }

    /// `verify()` must return `Err` when the challenge `c` is shorter than 32 bytes.
    ///
    /// The guard is `if self.c.len() != 32 { return Err(...) }` at line 426.
    /// A too-short challenge would otherwise panic at `self.c[..32].try_into()`.
    #[test]
    fn verify_rejects_short_challenge() {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let mut sig = MLSAGSignature::sign(b"msg", &signer_priv, &decoys)
            .expect("sign must succeed");

        // Replace the 32-byte challenge with a 16-byte truncation.
        sig.c = vec![0u8; 16];

        let result = sig.verify(b"msg");
        assert!(
            result.is_err(),
            "verify() must return Err for a challenge shorter than 32 bytes, got: {result:?}"
        );
    }

    /// Regression: `verify()` must return `Err` (not panic) when a response
    /// scalar `s[i]` has fewer than 32 bytes.
    ///
    /// Before the fix, `self.s[i][..32]` caused an index-out-of-bounds panic
    /// when `s[i].len() < 32`.  This is a consensus-path crash: a malicious
    /// peer can broadcast a block whose `RewardClaim` ring signature contains
    /// a short scalar, crashing any node that verifies the block.
    #[test]
    fn verify_rejects_short_response_scalar() {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let mut sig = MLSAGSignature::sign(b"msg", &signer_priv, &decoys)
            .expect("sign must succeed");
        // Replace first response scalar with 2 bytes — simulates attacker-
        // crafted data that would panic on `self.s[0][..32]` before the fix.
        sig.s[0] = vec![0x01, 0x02];

        let result = sig.verify(b"msg");
        assert!(
            result.is_err(),
            "verify must return Err for a short response scalar, \
             not panic with index-out-of-bounds"
        );
    }

    // ── #1735 hardening batch 9b: scalar-length boundary + MAX_RING_SIZE edge ──
    //
    // Complementing 9a, these tests pin two boundary cases for the fixes from
    // PR #1990: (1) a response scalar that is *longer* than 32 bytes (the
    // existing test only covers shorter), and (2) a ring of exactly MAX_RING_SIZE
    // members must NOT be rejected by the size guard (the guard is `>`, not `>=`).

    /// `verify()` must return `Err` when a response scalar `s[i]` has MORE than
    /// 32 bytes.
    ///
    /// The `as_slice().try_into::<[u8; 32]>()` call rejects slices that are not
    /// exactly 32 bytes — both too short AND too long.  This pins the too-long
    /// branch, which could indicate scalar malleability (prepended zero bytes etc.).
    #[test]
    fn verify_rejects_long_response_scalar() {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let mut sig = MLSAGSignature::sign(b"msg", &signer_priv, &decoys)
            .expect("sign must succeed");
        // Replace first response scalar with 33 bytes — simulates an attacker
        // prepending a zero byte for malleability or simply crafting a bad scalar.
        sig.s[0] = vec![0x00u8; 33];

        let result = sig.verify(b"msg");
        assert!(
            result.is_err(),
            "verify() must return Err for a long (33-byte) response scalar, got: {result:?}"
        );
    }

    /// A ring of exactly `MAX_RING_SIZE` members must NOT be rejected by the
    /// size guard in `verify()`.
    ///
    /// The guard is `n > MAX_RING_SIZE` (strict greater-than), so a ring of
    /// exactly MAX_RING_SIZE is allowed through.  This pins the boundary so
    /// that the `>` cannot be inadvertently changed to `>=` (which would break
    /// legitimate large privacy-critical transactions).
    #[test]
    fn verify_ring_at_max_size_not_rejected_by_size_guard() {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let mut sig = MLSAGSignature::sign(b"msg", &signer_priv, &decoys)
            .expect("sign must succeed");

        // Inflate ring and s to exactly MAX_RING_SIZE with dummy entries.
        // The signature will not cryptographically verify (the new members are
        // dummy), but the *size guard* must not fire.
        let extra = MAX_RING_SIZE - sig.ring.len();
        for _ in 0..extra {
            let (_, pub_key) = generate_keypair();
            sig.ring.push(RingMember { public_key: pub_key });
            sig.s.push(vec![0u8; 32]);
        }
        assert_eq!(sig.ring.len(), MAX_RING_SIZE, "ring must be exactly MAX_RING_SIZE");

        let result = sig.verify(b"msg");
        // The result may be Ok(false) or Err (bad crypto due to dummy members),
        // but must NOT be the "exceeds maximum" size-guard error.
        if let Err(PrivacyError::RingSignature(ref msg)) = result {
            assert!(
                !msg.contains("exceeds maximum"),
                "a ring of exactly MAX_RING_SIZE must pass the size guard \
                 (guard is `>`, not `>=`), got size-guard error: {msg}"
            );
        }
        // Any other result (Ok or non-size-guard Err) is acceptable.
    }

    /// Regression: `verify()` must return `Err` (not run O(n) EC operations)
    /// when the ring size exceeds `MAX_RING_SIZE`.
    ///
    /// An attacker can craft a peer-supplied signature with an arbitrarily
    /// large ring to force O(n) elliptic-curve multiplications per call and
    /// stall all verifying nodes.
    #[test]
    fn verify_rejects_ring_exceeding_max_size() {
        let (signer_priv, _) = generate_keypair();
        let decoys: Vec<RingMember> = (0..MIN_RING_SIZE - 1)
            .map(|_| {
                let (_, pub_key) = generate_keypair();
                RingMember { public_key: pub_key }
            })
            .collect();

        let mut sig = MLSAGSignature::sign(b"msg", &signer_priv, &decoys)
            .expect("sign must succeed");

        // Inflate ring and s to MAX_RING_SIZE + 1 with dummy values — this
        // simulates a signature crafted to exceed the bound.
        let extra = MAX_RING_SIZE + 1 - sig.ring.len();
        for _ in 0..extra {
            let (_, pub_key) = generate_keypair();
            sig.ring.push(RingMember { public_key: pub_key });
            sig.s.push(vec![0u8; 32]);
        }

        let result = sig.verify(b"msg");
        assert!(
            result.is_err(),
            "verify must return Err when ring size ({}) exceeds MAX_RING_SIZE ({})",
            sig.ring.len(),
            MAX_RING_SIZE
        );
        assert!(
            matches!(result.unwrap_err(), PrivacyError::RingSignature(_)),
            "error variant must be RingSignature"
        );
    }
}
