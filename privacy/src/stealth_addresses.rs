//! Stealth addresses for receiver privacy.
//!
//! A stealth address is a one-time address generated per transaction so that
//! the recipient's real public key never appears on chain.
//!
//! # Protocol
//!
//! 1. **Recipient** publishes two public keys: a *spend key* `B = bG` and a
//!    *view key* `A = aG`.
//! 2. **Sender** generates an ephemeral key pair `(r, R = rG)` and computes a
//!    shared secret `s = H(rA)`.
//! 3. **One-time public key**: `P = sG + B`.
//! 4. **Transaction key**: `R` is stored on-chain.
//! 5. **Recipient scans**: computes `P' = H(aR)G + B` and checks `P' == P`.
//! 6. **Recipient spends**: recovers the private key `x = H(aR) + b`.
//!
//! This module uses Ristretto255 via `curve25519-dalek`.

use crate::error::{PrivacyError, PrivacyResult};
use curve25519_dalek::{
    constants::RISTRETTO_BASEPOINT_TABLE,
    ristretto::CompressedRistretto,
    scalar::Scalar,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

// ---------------------------------------------------------------------------
// StealthKeys
// ---------------------------------------------------------------------------

/// A complete set of stealth keys for a participant.
///
/// The *view* key pair allows scanning the chain for incoming payments without
/// the ability to spend.  The *spend* key pair is required to actually move
/// funds.
/// `Debug` is hand-implemented to redact the private keys, and `Serialize`
/// and `Clone` are deliberately not implemented: a derived `Serialize`
/// would emit both private keys into any log line, panic message, or JSON
/// dump that touches the value, and a derived `Clone` silently multiplies
/// unzeroized copies of the key material in ordinary memory (#2259, #2266
/// review). Callers that need to export key material must do so
/// field-by-field, explicitly.
pub struct StealthKeys {
    /// Public spend key (`B = b*G`).
    pub public_spend_key: [u8; 32],
    /// Public view key (`A = a*G`).
    pub public_view_key: [u8; 32],
    /// Private spend key (`b`).
    pub private_spend_key: [u8; 32],
    /// Private view key (`a`).
    pub private_view_key: [u8; 32],
}

impl std::fmt::Debug for StealthKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StealthKeys")
            .field("public_spend_key", &hex::encode(self.public_spend_key))
            .field("public_view_key", &hex::encode(self.public_view_key))
            .field("private_spend_key", &"<redacted>")
            .field("private_view_key", &"<redacted>")
            .finish()
    }
}

impl StealthKeys {
    /// Generate a fresh stealth key set from the OS CSPRNG.
    ///
    /// ```
    /// use wrfcoin_privacy::stealth_addresses::StealthKeys;
    ///
    /// let keys = StealthKeys::generate();
    /// // The public keys are valid compressed Ristretto points.
    /// assert_ne!(keys.public_spend_key, [0u8; 32]);
    /// assert_ne!(keys.public_view_key, [0u8; 32]);
    /// ```
    pub fn generate() -> Self {
        let mut rng = OsRng;

        // Generate random scalars for spend and view keys.
        let mut spend_bytes = [0u8; 64];
        let mut view_bytes = [0u8; 64];
        rng.fill_bytes(&mut spend_bytes);
        rng.fill_bytes(&mut view_bytes);

        let spend_scalar = Scalar::from_bytes_mod_order_wide(&spend_bytes);
        let view_scalar = Scalar::from_bytes_mod_order_wide(&view_bytes);

        // Derive the corresponding public keys.
        let public_spend = (&spend_scalar * RISTRETTO_BASEPOINT_TABLE)
            .compress()
            .to_bytes();
        let public_view = (&view_scalar * RISTRETTO_BASEPOINT_TABLE)
            .compress()
            .to_bytes();

        Self {
            public_spend_key: public_spend,
            public_view_key: public_view,
            private_spend_key: spend_scalar.to_bytes(),
            private_view_key: view_scalar.to_bytes(),
        }
    }

    /// Return the publishable stealth-address format string `SSF<spend><view>`.
    pub fn stealth_address_format(&self) -> String {
        format!(
            "SSF{}{}",
            hex::encode(self.public_spend_key),
            hex::encode(self.public_view_key),
        )
    }
}

// ---------------------------------------------------------------------------
// StealthAddress
// ---------------------------------------------------------------------------

/// A one-time stealth address derived for a single transaction.
///
/// The struct contains all on-chain data needed for the recipient to detect
/// and spend the payment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StealthAddress {
    /// Recipient's public spend key `B`.
    pub public_spend_key: [u8; 32],
    /// Recipient's public view key `A`.
    pub public_view_key: [u8; 32],
    /// Derived one-time public key `P = H(rA)G + B`.
    pub one_time_public_key: [u8; 32],
    /// Sender's ephemeral public key `R = rG` (stored on-chain).
    pub transaction_key: [u8; 32],
    /// Optional 8-byte payment identifier (XOR-encrypted with shared secret).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_id: Option<[u8; 8]>,
}

impl StealthAddress {
    /// Derive a new one-time stealth address for a recipient.
    ///
    /// The caller provides the recipient's public spend and view keys and an
    /// optional payment ID.  Returns `(stealth_address, ephemeral_secret)`.
    ///
    /// The ephemeral secret is **not** stored on-chain; the sender may discard
    /// it after broadcasting.
    pub fn new(
        public_spend_key: [u8; 32],
        public_view_key: [u8; 32],
        payment_id: Option<[u8; 8]>,
    ) -> PrivacyResult<(Self, [u8; 32])> {
        // Generate ephemeral key pair (r, R = rG).
        let mut rng = OsRng;
        let mut ephemeral_bytes = [0u8; 64];
        rng.fill_bytes(&mut ephemeral_bytes);
        let ephemeral_scalar = Scalar::from_bytes_mod_order_wide(&ephemeral_bytes);
        let ephemeral_public = (&ephemeral_scalar * RISTRETTO_BASEPOINT_TABLE)
            .compress()
            .to_bytes();

        // Decompress recipient's view key A.
        let view_point = decompress_point(&public_view_key)
            .ok_or_else(|| PrivacyError::InvalidKey("invalid public view key".into()))?;

        // Decompress recipient's spend key B.
        let spend_point = decompress_point(&public_spend_key)
            .ok_or_else(|| PrivacyError::InvalidKey("invalid public spend key".into()))?;

        // Shared secret s = H(r * A).
        let shared_point = ephemeral_scalar * view_point;
        let shared_secret: [u8; 32] =
            Sha3_256::digest(shared_point.compress().as_bytes()).into();
        let derived_scalar = Scalar::from_bytes_mod_order(shared_secret);

        // One-time public key P = s*G + B.
        let one_time_point =
            (&derived_scalar * RISTRETTO_BASEPOINT_TABLE) + spend_point;
        let one_time_public_key = one_time_point.compress().to_bytes();

        // Encrypt payment ID with first 8 bytes of shared secret (simple XOR).
        let encrypted_payment_id = payment_id.map(|id| {
            let mut encrypted = id;
            for i in 0..8 {
                encrypted[i] ^= shared_secret[i];
            }
            encrypted
        });

        Ok((
            Self {
                public_spend_key,
                public_view_key,
                one_time_public_key,
                transaction_key: ephemeral_public,
                payment_id: encrypted_payment_id,
            },
            ephemeral_scalar.to_bytes(),
        ))
    }

    /// Check whether this stealth address was generated for a given recipient.
    ///
    /// The recipient provides their private view key `a` and public spend key
    /// `B`.  The method recomputes the expected one-time key `P' = H(aR)G + B`
    /// and compares it against the stored `one_time_public_key`.
    pub fn is_for_recipient(
        &self,
        private_view_key: &[u8; 32],
        public_spend_key: &[u8; 32],
    ) -> PrivacyResult<bool> {
        // Decompress the transaction key R.
        let tx_point = decompress_point(&self.transaction_key)
            .ok_or_else(|| PrivacyError::InvalidKey("invalid transaction key".into()))?;

        // Decompress recipient's spend key B.
        let spend_point = decompress_point(public_spend_key)
            .ok_or_else(|| PrivacyError::InvalidKey("invalid public spend key".into()))?;

        let view_scalar = Scalar::from_bytes_mod_order(*private_view_key);

        // Shared secret s = H(a * R).
        let shared_point = view_scalar * tx_point;
        let shared_secret: [u8; 32] =
            Sha3_256::digest(shared_point.compress().as_bytes()).into();
        let derived_scalar = Scalar::from_bytes_mod_order(shared_secret);

        // Expected one-time key P' = s*G + B.
        let expected_point =
            (&derived_scalar * RISTRETTO_BASEPOINT_TABLE) + spend_point;
        let expected_bytes = expected_point.compress().to_bytes();

        Ok(expected_bytes == self.one_time_public_key)
    }

    /// Recover the one-time private key for spending.
    ///
    /// The recipient computes `x = H(aR) + b` which corresponds to the
    /// one-time public key `P = H(aR)G + B`.
    pub fn recover_private_key(
        &self,
        private_view_key: &[u8; 32],
        private_spend_key: &[u8; 32],
    ) -> PrivacyResult<[u8; 32]> {
        // Decompress the transaction key R.
        let tx_point = decompress_point(&self.transaction_key)
            .ok_or_else(|| PrivacyError::InvalidKey("invalid transaction key".into()))?;

        let view_scalar = Scalar::from_bytes_mod_order(*private_view_key);
        let spend_scalar = Scalar::from_bytes_mod_order(*private_spend_key);

        // Shared secret s = H(a * R).
        let shared_point = view_scalar * tx_point;
        let shared_secret: [u8; 32] =
            Sha3_256::digest(shared_point.compress().as_bytes()).into();
        let derived_scalar = Scalar::from_bytes_mod_order(shared_secret);

        // One-time private key x = s + b.
        let one_time_private = (derived_scalar + spend_scalar).to_bytes();

        Ok(one_time_private)
    }

    /// Decrypt the payment ID using the recipient's private view key.
    ///
    /// Returns `Ok(None)` when no payment ID is attached.
    pub fn decrypt_payment_id(
        &self,
        private_view_key: &[u8; 32],
    ) -> PrivacyResult<Option<[u8; 8]>> {
        let encrypted = match self.payment_id {
            Some(id) => id,
            None => return Ok(None),
        };

        let tx_point = decompress_point(&self.transaction_key)
            .ok_or_else(|| PrivacyError::InvalidKey("invalid transaction key".into()))?;

        let view_scalar = Scalar::from_bytes_mod_order(*private_view_key);
        let shared_point = view_scalar * tx_point;
        let shared_secret: [u8; 32] =
            Sha3_256::digest(shared_point.compress().as_bytes()).into();

        let mut decrypted = [0u8; 8];
        for i in 0..8 {
            decrypted[i] = encrypted[i] ^ shared_secret[i];
        }

        Ok(Some(decrypted))
    }

    /// Serialize to a JSON string.
    pub fn to_json(&self) -> PrivacyResult<String> {
        serde_json::to_string(self).map_err(PrivacyError::from)
    }

    /// Deserialize from a JSON string.
    pub fn from_json(json: &str) -> PrivacyResult<Self> {
        serde_json::from_str(json).map_err(PrivacyError::from)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Decompress a 32-byte compressed Ristretto point.
fn decompress_point(
    bytes: &[u8; 32],
) -> Option<curve25519_dalek::ristretto::RistrettoPoint> {
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

    #[test]
    fn generate_keys_produces_valid_points() {
        let keys = StealthKeys::generate();
        // Public keys must decompress to valid Ristretto points.
        assert!(decompress_point(&keys.public_spend_key).is_some());
        assert!(decompress_point(&keys.public_view_key).is_some());
    }

    #[test]
    fn stealth_address_roundtrip() {
        let recipient = StealthKeys::generate();
        let (addr, _ephemeral) = StealthAddress::new(
            recipient.public_spend_key,
            recipient.public_view_key,
            None,
        )
        .unwrap();

        // Recipient can detect the payment.
        assert!(addr
            .is_for_recipient(&recipient.private_view_key, &recipient.public_spend_key)
            .unwrap());

        // Recipient can recover the private key.
        let recovered = addr
            .recover_private_key(&recipient.private_view_key, &recipient.private_spend_key)
            .unwrap();

        // The recovered private key times G should equal the one-time public key.
        let recovered_scalar = Scalar::from_bytes_mod_order(recovered);
        let recovered_public = (&recovered_scalar * RISTRETTO_BASEPOINT_TABLE)
            .compress()
            .to_bytes();
        assert_eq!(recovered_public, addr.one_time_public_key);
    }

    #[test]
    fn wrong_recipient_cannot_detect() {
        let recipient = StealthKeys::generate();
        let bystander = StealthKeys::generate();

        let (addr, _) = StealthAddress::new(
            recipient.public_spend_key,
            recipient.public_view_key,
            None,
        )
        .unwrap();

        // Bystander should not detect the payment.
        assert!(!addr
            .is_for_recipient(&bystander.private_view_key, &bystander.public_spend_key)
            .unwrap());
    }

    #[test]
    fn payment_id_encryption_roundtrip() {
        let recipient = StealthKeys::generate();
        let payment_id: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];

        let (addr, _) = StealthAddress::new(
            recipient.public_spend_key,
            recipient.public_view_key,
            Some(payment_id),
        )
        .unwrap();

        // The stored payment ID should be encrypted (different from plaintext).
        assert_ne!(addr.payment_id.unwrap(), payment_id);

        // Recipient can decrypt it.
        let decrypted = addr
            .decrypt_payment_id(&recipient.private_view_key)
            .unwrap()
            .unwrap();
        assert_eq!(decrypted, payment_id);
    }

    #[test]
    fn no_payment_id_returns_none() {
        let recipient = StealthKeys::generate();
        let (addr, _) = StealthAddress::new(
            recipient.public_spend_key,
            recipient.public_view_key,
            None,
        )
        .unwrap();
        assert_eq!(addr.decrypt_payment_id(&recipient.private_view_key).unwrap(), None); // secret-leak-ok: test-only fixture key, never a production output path
    }

    #[test]
    fn json_roundtrip() {
        let recipient = StealthKeys::generate();
        let (addr, _) = StealthAddress::new(
            recipient.public_spend_key,
            recipient.public_view_key,
            Some([0xAA; 8]),
        )
        .unwrap();

        let json = addr.to_json().unwrap();
        let recovered = StealthAddress::from_json(&json).unwrap();
        assert_eq!(addr, recovered);
    }

    #[test]
    fn debug_redacts_private_keys() {
        let keys = StealthKeys::generate();
        let rendered = format!("{keys:?}");

        // Public keys are visible; private key material must never appear in
        // Debug output in any encoding we emit (#2259).
        assert!(rendered.contains(&hex::encode(keys.public_spend_key)));
        assert!(rendered.contains(&hex::encode(keys.public_view_key)));
        assert!(rendered.contains("<redacted>"));
        // secret-leak-ok: test verifies redaction; assert! failure prints only
        // the expression source, never the evaluated key material.
        assert!(!rendered.contains(&hex::encode(keys.private_spend_key))); // secret-leak-ok: see above
        assert!(!rendered.contains(&hex::encode(keys.private_view_key))); // secret-leak-ok: see above
        assert!(!rendered.contains(&format!("{:?}", keys.private_spend_key))); // secret-leak-ok: see above
        assert!(!rendered.contains(&format!("{:?}", keys.private_view_key))); // secret-leak-ok: see above
    }

    #[test]
    fn stealth_address_format() {
        let keys = StealthKeys::generate();
        let format = keys.stealth_address_format();
        assert!(format.starts_with("SSF"));
        // 3 prefix chars + 64 hex chars (spend) + 64 hex chars (view) = 131.
        assert_eq!(format.len(), 3 + 64 + 64);
    }

    #[test]
    fn invalid_key_rejected() {
        use curve25519_dalek::ristretto::CompressedRistretto;

        // Find a 32-byte value that does NOT decompress to a valid Ristretto
        // point.  Most random 32-byte values are not valid Ristretto encodings.
        let mut bad = [0u8; 32];
        for candidate in 1u8..=255 {
            bad[0] = candidate;
            if CompressedRistretto::from_slice(&bad)
                .ok()
                .and_then(|c| c.decompress())
                .is_none()
            {
                // Found an invalid compressed point.
                let result = StealthAddress::new(bad, bad, None);
                assert!(result.is_err(), "expected invalid key rejection");
                return;
            }
        }
        panic!("could not find invalid Ristretto point (unexpected)");
    }

    #[test]
    fn ristretto_key_roundtrip_with_recovery() {
        let recipient = StealthKeys::generate();
        let (addr, _) = StealthAddress::new(
            recipient.public_spend_key,
            recipient.public_view_key,
            None,
        )
        .unwrap();

        // Recipient detects the payment.
        assert!(addr
            .is_for_recipient(&recipient.private_view_key, &recipient.public_spend_key)
            .unwrap());

        // Recover private key and verify against the Ristretto basepoint.
        let recovered = addr
            .recover_private_key(&recipient.private_view_key, &recipient.private_spend_key)
            .unwrap();
        let recovered_scalar = Scalar::from_bytes_mod_order(recovered);
        let recovered_public = (&recovered_scalar * RISTRETTO_BASEPOINT_TABLE)
            .compress()
            .to_bytes();
        assert_eq!(recovered_public, addr.one_time_public_key);
    }

    #[test]
    fn each_invocation_produces_unique_address() {
        let recipient = StealthKeys::generate();
        let (addr1, _) = StealthAddress::new(
            recipient.public_spend_key,
            recipient.public_view_key,
            None,
        )
        .unwrap();
        let (addr2, _) = StealthAddress::new(
            recipient.public_spend_key,
            recipient.public_view_key,
            None,
        )
        .unwrap();
        // Different ephemeral keys produce different one-time addresses.
        assert_ne!(addr1.one_time_public_key, addr2.one_time_public_key);
        assert_ne!(addr1.transaction_key, addr2.transaction_key);
    }
}
