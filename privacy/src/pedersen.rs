//! Pedersen commitments for confidential transactions.
//!
//! A Pedersen commitment `C = vG + rH` hides a value `v` behind a blinding
//! factor `r` while preserving additive homomorphism: `C(a) + C(b) = C(a+b)`.
//!
//! # Overview
//!
//! The commitment scheme uses two independent generators on the Ristretto group:
//!
//! - **G** -- the standard Ristretto basepoint.
//! - **H** -- derived by hashing the basepoint (nothing-up-my-sleeve construction).
//!
//! # Example
//!
//! ```rust
//! use wrfcoin_privacy::pedersen::PedersenCommitment;
//!
//! // Commit to a value with a random blinding factor.
//! let (commitment, blinding) = PedersenCommitment::commit(42);
//! assert!(commitment.verify(42, &blinding));
//! assert!(!commitment.verify(43, &blinding));
//! ```

use std::ops::Add;

use crate::error::{PrivacyError, PrivacyResult};
use curve25519_dalek::{
    constants::RISTRETTO_BASEPOINT_TABLE,
    ristretto::{CompressedRistretto, RistrettoPoint},
    scalar::Scalar,
};
use rand_core::{OsRng, RngCore};
use sha3::{Digest, Sha3_512};
use zeroize::Zeroize;

// ---------------------------------------------------------------------------
// Generator H
// ---------------------------------------------------------------------------

/// Compute the secondary generator **H** for Pedersen commitments.
///
/// H is derived deterministically from the standard Ristretto basepoint G by
/// hashing it through SHA3-512 and mapping the full 64-byte output to a
/// uniform Ristretto point.  This is a standard nothing-up-my-sleeve
/// construction: nobody knows the discrete log of H with respect to G.
///
/// We use SHA3-512 (not SHA3-256) because `RistrettoPoint::from_uniform_bytes`
/// requires 64 bytes of uniform randomness to produce a uniformly distributed
/// point on the Ristretto group at full 128-bit security strength.
fn h_generator() -> RistrettoPoint {
    let mut hasher = Sha3_512::new();
    hasher.update(b"WRFCOIN_PEDERSEN_H_GENERATOR");
    hasher.update(RISTRETTO_BASEPOINT_TABLE.basepoint().compress().as_bytes());
    let hash = hasher.finalize();
    let mut uniform = [0u8; 64];
    uniform.copy_from_slice(&hash);
    RistrettoPoint::from_uniform_bytes(&uniform)
}

// ---------------------------------------------------------------------------
// PedersenCommitment
// ---------------------------------------------------------------------------

/// A Pedersen commitment to a scalar value.
///
/// The commitment point is `C = v*G + r*H` where `v` is the committed value
/// and `r` is the blinding factor.  Both generators live on the Ristretto255
/// group.
#[derive(Debug, Clone)]
pub struct PedersenCommitment {
    /// The commitment point on the Ristretto group.
    pub commitment: RistrettoPoint,
    /// The blinding factor used to hide the value.
    pub blinding_factor: Scalar,
}

impl PartialEq for PedersenCommitment {
    fn eq(&self, other: &Self) -> bool {
        self.commitment.compress().as_bytes() == other.commitment.compress().as_bytes()
            && self.blinding_factor.as_bytes() == other.blinding_factor.as_bytes()
    }
}

impl Eq for PedersenCommitment {}

impl PedersenCommitment {
    /// Create a commitment to `value` with the given `blinding` factor.
    ///
    /// ```
    /// use curve25519_dalek::scalar::Scalar;
    /// use wrfcoin_privacy::pedersen::PedersenCommitment;
    ///
    /// let blinding = Scalar::from(99u64);
    /// let c = PedersenCommitment::new(100, &blinding);
    /// assert!(c.verify(100, &blinding));
    /// ```
    pub fn new(value: u64, blinding: &Scalar) -> Self {
        let v = Scalar::from(value);
        let h = h_generator();
        // C = v*G + r*H
        let commitment = &v * RISTRETTO_BASEPOINT_TABLE + blinding * h;
        Self {
            commitment,
            blinding_factor: *blinding,
        }
    }

    /// Create a commitment to `value` with a cryptographically random blinding
    /// factor.  Returns the commitment and the generated blinding scalar.
    ///
    /// ```
    /// use wrfcoin_privacy::pedersen::PedersenCommitment;
    ///
    /// let (c, blinding) = PedersenCommitment::commit(500);
    /// assert!(c.verify(500, &blinding));
    /// ```
    pub fn commit(value: u64) -> (Self, Scalar) {
        let mut blinding_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut blinding_bytes);
        let blinding = Scalar::from_bytes_mod_order(blinding_bytes);
        let commitment = Self::new(value, &blinding);
        (commitment, blinding)
    }

    /// Verify that this commitment opens to `value` with the given `blinding`
    /// factor.
    ///
    /// Returns `true` when `C == value*G + blinding*H`.
    pub fn verify(&self, value: u64, blinding: &Scalar) -> bool {
        let v = Scalar::from(value);
        let h = h_generator();
        let expected = &v * RISTRETTO_BASEPOINT_TABLE + blinding * h;
        expected == self.commitment
    }

    /// Serialize the commitment point to 32 bytes (compressed Ristretto).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.commitment.compress().as_bytes().to_vec()
    }

    /// Deserialize a commitment point from 32 bytes of compressed Ristretto.
    ///
    /// Note: this recovers only the commitment *point*, not the blinding
    /// factor.  The blinding factor is set to zero; the caller must track it
    /// separately if needed.
    pub fn from_bytes(bytes: &[u8]) -> PrivacyResult<Self> {
        if bytes.len() != 32 {
            return Err(PrivacyError::InvalidCommitment(format!(
                "expected 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(bytes);
        let point = CompressedRistretto(array)
            .decompress()
            .ok_or_else(|| PrivacyError::InvalidCommitment("invalid Ristretto point".into()))?;
        Ok(Self {
            commitment: point,
            blinding_factor: Scalar::ZERO,
        })
    }
}

// ---------------------------------------------------------------------------
// Homomorphic addition: C(a,r1) + C(b,r2) = C(a+b, r1+r2)
// ---------------------------------------------------------------------------

impl Add for PedersenCommitment {
    type Output = Self;

    /// Add two commitments homomorphically.
    ///
    /// If `lhs = Commit(a, r1)` and `rhs = Commit(b, r2)`, then
    /// `lhs + rhs = Commit(a + b, r1 + r2)`.
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            commitment: self.commitment + rhs.commitment,
            blinding_factor: self.blinding_factor + rhs.blinding_factor,
        }
    }
}

impl<'a> Add<&'a PedersenCommitment> for &'a PedersenCommitment {
    type Output = PedersenCommitment;

    fn add(self, rhs: &'a PedersenCommitment) -> Self::Output {
        PedersenCommitment {
            commitment: self.commitment + rhs.commitment,
            blinding_factor: self.blinding_factor + rhs.blinding_factor,
        }
    }
}

// ---------------------------------------------------------------------------
// Serde: hex-encoded compressed point + blinding factor
// ---------------------------------------------------------------------------

impl serde::Serialize for PedersenCommitment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let commitment_hex = hex::encode(self.commitment.compress().as_bytes());
        let blinding_hex = hex::encode(self.blinding_factor.as_bytes());

        let mut state = serializer.serialize_struct("PedersenCommitment", 2)?;
        state.serialize_field("commitment", &commitment_hex)?;
        state.serialize_field("blinding_factor", &blinding_hex)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for PedersenCommitment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct PedersenVisitor;

        impl<'de> Visitor<'de> for PedersenVisitor {
            type Value = PedersenCommitment;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a PedersenCommitment with hex-encoded fields")
            }

            #[allow(clippy::excessive_nesting)]
            fn visit_map<M>(self, mut map: M) -> Result<PedersenCommitment, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut commitment_hex: Option<String> = None;
                let mut blinding_hex: Option<String> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "commitment" => {
                            if commitment_hex.is_some() {
                                return Err(de::Error::duplicate_field("commitment"));
                            }
                            commitment_hex = Some(map.next_value()?);
                        }
                        "blinding_factor" => {
                            if blinding_hex.is_some() {
                                return Err(de::Error::duplicate_field("blinding_factor"));
                            }
                            blinding_hex = Some(map.next_value()?);
                        }
                        _ => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }

                let commitment_bytes = hex::decode(
                    commitment_hex
                        .ok_or_else(|| de::Error::missing_field("commitment"))?,
                )
                .map_err(|e| de::Error::custom(format!("invalid commitment hex: {e}")))?;

                let blinding_bytes = hex::decode(
                    blinding_hex
                        .ok_or_else(|| de::Error::missing_field("blinding_factor"))?,
                )
                .map_err(|e| de::Error::custom(format!("invalid blinding_factor hex: {e}")))?;

                if commitment_bytes.len() != 32 {
                    return Err(de::Error::custom("commitment must be 32 bytes"));
                }
                if blinding_bytes.len() != 32 {
                    return Err(de::Error::custom("blinding_factor must be 32 bytes"));
                }

                let mut commitment_array = [0u8; 32];
                commitment_array.copy_from_slice(&commitment_bytes);
                let point = CompressedRistretto(commitment_array)
                    .decompress()
                    .ok_or_else(|| de::Error::custom("invalid Ristretto point"))?;

                let mut blinding_array = [0u8; 32];
                blinding_array.copy_from_slice(&blinding_bytes);
                let blinding = Scalar::from_bytes_mod_order(blinding_array);

                Ok(PedersenCommitment {
                    commitment: point,
                    blinding_factor: blinding,
                })
            }
        }

        const FIELDS: &[&str] = &["commitment", "blinding_factor"];
        deserializer.deserialize_struct("PedersenCommitment", FIELDS, PedersenVisitor)
    }
}

// ---------------------------------------------------------------------------
// ConfidentialAsset
// ---------------------------------------------------------------------------

/// A confidential asset wraps a Pedersen commitment with asset-type metadata
/// and an optional encrypted amount for authorized viewers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfidentialAsset {
    /// Pedersen commitment to the amount.
    pub commitment: PedersenCommitment,
    /// Plaintext asset type identifier (e.g. `"WRF"`, `"CARBON"`).
    pub asset_type: String,
    /// Optionally encrypted amount blob for the recipient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_amount: Option<Vec<u8>>,
}

impl ConfidentialAsset {
    /// Create a new confidential asset with a random blinding factor.
    ///
    /// Returns the asset and the blinding scalar (needed for spending later).
    pub fn new(asset_type: impl Into<String>, amount: u64) -> (Self, Scalar) {
        let (commitment, blinding) = PedersenCommitment::commit(amount);
        let asset = Self {
            commitment,
            asset_type: asset_type.into(),
            encrypted_amount: None,
        };
        (asset, blinding)
    }

    /// Create a confidential asset with an explicit blinding factor.
    pub fn with_blinding(
        asset_type: impl Into<String>,
        amount: u64,
        blinding: &Scalar,
    ) -> Self {
        let commitment = PedersenCommitment::new(amount, blinding);
        Self {
            commitment,
            asset_type: asset_type.into(),
            encrypted_amount: None,
        }
    }

    /// Attach an encrypted amount blob (e.g. encrypted with the recipient's
    /// public key) so authorized viewers can decrypt the actual amount.
    pub fn with_encrypted_amount(mut self, encrypted: Vec<u8>) -> Self {
        self.encrypted_amount = Some(encrypted);
        self
    }

    /// Verify the committed amount given the blinding factor.
    pub fn verify_amount(&self, amount: u64, blinding: &Scalar) -> bool {
        self.commitment.verify(amount, blinding)
    }
}

// ---------------------------------------------------------------------------
// Drop: zeroize the blinding factor
// ---------------------------------------------------------------------------

impl Drop for PedersenCommitment {
    fn drop(&mut self) {
        self.blinding_factor.zeroize();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_and_verify() {
        let (c, blinding) = PedersenCommitment::commit(100);
        assert!(c.verify(100, &blinding));
        assert!(!c.verify(101, &blinding));
    }

    #[test]
    fn new_with_explicit_blinding() {
        let blinding = Scalar::from(42u64);
        let c = PedersenCommitment::new(999, &blinding);
        assert!(c.verify(999, &blinding));
    }

    #[test]
    fn homomorphic_addition() {
        let (c1, b1) = PedersenCommitment::commit(30);
        let (c2, b2) = PedersenCommitment::commit(70);
        let sum = c1 + c2;
        let blinding_sum = b1 + b2;
        assert!(sum.verify(100, &blinding_sum));
        assert!(!sum.verify(99, &blinding_sum));
    }

    #[test]
    fn homomorphic_addition_ref() {
        let (c1, b1) = PedersenCommitment::commit(10);
        let (c2, b2) = PedersenCommitment::commit(20);
        let sum = &c1 + &c2;
        let blinding_sum = b1 + b2;
        assert!(sum.verify(30, &blinding_sum));
    }

    #[test]
    fn to_bytes_roundtrip() {
        let (c, _) = PedersenCommitment::commit(42);
        let bytes = c.to_bytes();
        assert_eq!(bytes.len(), 32);
        let recovered = PedersenCommitment::from_bytes(&bytes).unwrap();
        assert_eq!(
            c.commitment.compress().as_bytes(),
            recovered.commitment.compress().as_bytes()
        );
    }

    #[test]
    fn from_bytes_rejects_bad_length() {
        assert!(PedersenCommitment::from_bytes(&[0u8; 31]).is_err());
        assert!(PedersenCommitment::from_bytes(&[0u8; 33]).is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let (c, _) = PedersenCommitment::commit(123);
        let json = serde_json::to_string(&c).unwrap();
        let recovered: PedersenCommitment = serde_json::from_str(&json).unwrap();
        assert_eq!(c, recovered);
    }

    #[test]
    fn confidential_asset_basic() {
        let (asset, blinding) = ConfidentialAsset::new("WRF", 500);
        assert_eq!(asset.asset_type, "WRF");
        assert!(asset.verify_amount(500, &blinding));
        assert!(!asset.verify_amount(501, &blinding));
        assert!(asset.encrypted_amount.is_none());
    }

    #[test]
    fn confidential_asset_with_encrypted_amount() {
        let (asset, _) = ConfidentialAsset::new("CARBON", 1000);
        let asset = asset.with_encrypted_amount(vec![0xDE, 0xAD]);
        assert_eq!(asset.encrypted_amount, Some(vec![0xDE, 0xAD]));
    }

    #[test]
    fn h_generator_is_valid_and_independent() {
        let h = h_generator();
        let g = RISTRETTO_BASEPOINT_TABLE.basepoint();
        let identity = RistrettoPoint::default();

        // H must not be the identity element
        assert_ne!(
            h.compress().as_bytes(),
            identity.compress().as_bytes(),
            "H generator must not be the identity"
        );

        // H must not equal G (otherwise commitments are trivially openable)
        assert_ne!(
            h.compress().as_bytes(),
            g.compress().as_bytes(),
            "H generator must differ from basepoint G"
        );

        // H must be deterministic
        let h2 = h_generator();
        assert_eq!(
            h.compress().as_bytes(),
            h2.compress().as_bytes(),
            "H generator must be deterministic"
        );
    }

    #[test]
    fn confidential_asset_serde_roundtrip() {
        let (asset, _) = ConfidentialAsset::new("WRF", 250);
        let json = serde_json::to_string(&asset).unwrap();
        let recovered: ConfidentialAsset = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.asset_type, "WRF");
        assert_eq!(
            asset.commitment.commitment.compress().as_bytes(),
            recovered.commitment.commitment.compress().as_bytes()
        );
    }
}
