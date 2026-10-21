use crate::crypto::hash::Hash;
use anyhow::{Result, anyhow};
use ed25519_dalek::{Signature as Ed25519Signature, Signer, Verifier};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::hash::{Hash as StdHash, Hasher};

/// A digital signature
#[derive(Clone, PartialEq, Eq)]
pub struct Signature(Ed25519Signature);

/// A public key for verifying signatures
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PublicKey(ed25519_dalek::VerifyingKey);

/// A private key for creating signatures
#[derive(Clone)] // secret-leak-ok: required by KeyPair's public Clone API; copies stay in-process and PrivateKey Debug is redacted
pub struct PrivateKey(ed25519_dalek::SigningKey);

/// A keypair containing both private and public keys
#[derive(Debug, Clone)]
pub struct KeyPair {
    pub private_key: PrivateKey,
    pub public_key: PublicKey,
}

impl PartialEq for KeyPair {
    fn eq(&self, other: &Self) -> bool {
        self.public_key == other.public_key
    }
}

impl Eq for KeyPair {}

impl StdHash for KeyPair {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Only hash the public key since it uniquely identifies the key pair
        self.public_key.to_hex().hash(state);
    }
}

impl Signature {
    /// Creates a new signature from raw bytes
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != ed25519_dalek::SIGNATURE_LENGTH {
            return Err(anyhow!("Invalid signature length"));
        }

        let mut sig_bytes = [0u8; ed25519_dalek::SIGNATURE_LENGTH];
        sig_bytes.copy_from_slice(bytes);

        // In ed25519-dalek v2.0, from_bytes returns the Signature directly, not a Result
        let ed_sig = Ed25519Signature::from_bytes(&sig_bytes);
        Ok(Self(ed_sig))
    }

    /// Gets the byte representation of the signature
    pub fn as_bytes(&self) -> [u8; ed25519_dalek::SIGNATURE_LENGTH] {
        self.0.to_bytes()
    }

    /// Converts the signature to a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// Creates a signature from a hex string
    #[inline]
    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let bytes = hex::decode(hex_str)?;
        Self::from_bytes(&bytes)
    }
}

impl PublicKey {
    /// Creates a public key from raw bytes
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != ed25519_dalek::PUBLIC_KEY_LENGTH {
            return Err(anyhow!("Invalid public key length"));
        }

        let mut pk_bytes = [0u8; ed25519_dalek::PUBLIC_KEY_LENGTH];
        pk_bytes.copy_from_slice(bytes);

        match ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes) {
            Ok(pk) => Ok(Self(pk)),
            Err(e) => Err(anyhow!("Invalid public key: {e}")),
        }
    }

    /// Derives a public key from a private key
    pub fn from_private_key(private_key: &PrivateKey) -> Self {
        Self(private_key.0.verifying_key())
    }

    /// Gets the byte representation of the public key
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Converts the public key to a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// Creates a public key from a hex string
    #[inline]
    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let bytes = hex::decode(hex_str)?;
        Self::from_bytes(&bytes)
    }

    /// Verifies a signature against a message.
    ///
    /// #2043: uses ed25519-dalek's `verify_strict`, not the permissive
    /// `Verifier::verify`. Strict verification rejects non-canonical `S`
    /// (`S >= L`), non-canonical `R`, and small-order public keys — closing the
    /// signature-malleability surface. Because WRFCoin's tx-id
    /// (`calculate_hash`) folds in the input signature while `signing_hash`
    /// excludes it, a malleated signature would otherwise yield a *different*
    /// tx-id for the same logical transfer (txid-malleability). Honestly
    /// produced ed25519 signatures are always canonical, so this does not reject
    /// any legitimately signed object.
    #[inline]
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        self.0.verify_strict(message, &signature.0).is_ok()
    }

    /// Verifies a signature (as bytes) against a message
    pub fn verify_signature(&self, message: &[u8], signature_bytes: &[u8]) -> bool {
        // Parse the signature
        if signature_bytes.len() != ed25519_dalek::SIGNATURE_LENGTH {
            return false;
        }

        // Create a fixed-size array for the signature
        let mut sig_bytes = [0u8; ed25519_dalek::SIGNATURE_LENGTH];
        sig_bytes.copy_from_slice(signature_bytes);

        // First try to create a signature from the bytes
        match ed25519_dalek::Signature::try_from(sig_bytes.as_ref()) {
            Ok(signature) => {
                // #2043: strict verification (see `verify`) — rejects
                // non-canonical / malleable encodings.
                self.0.verify_strict(message, &signature).is_ok()
            },
            Err(_) => false,
        }
    }

    /// Derives a wallet address from this public key
    pub fn to_address(&self) -> String {
        // Use the first 20 bytes of the hash of the public key as the address
        let pk_hash = Hash::hash_bytes(self.as_bytes());
        format!("wrf{}", hex::encode(&pk_hash.as_bytes()[0..20]))
    }
}

impl PrivateKey {
    /// Creates a new random private key
    pub fn generate<R: rand::CryptoRng + rand::RngCore>(csprng: &mut R) -> Self {
        let signing_key = ed25519_dalek::SigningKey::generate(csprng);
        Self(signing_key)
    }

    /// Creates a private key from raw bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != ed25519_dalek::SECRET_KEY_LENGTH {
            return Err(anyhow!("Invalid private key length"));
        }

        let mut sk_bytes = [0u8; ed25519_dalek::SECRET_KEY_LENGTH];
        sk_bytes.copy_from_slice(bytes);

        let signing_key = ed25519_dalek::SigningKey::from_bytes(&sk_bytes);
        Ok(Self(signing_key))
    }

    /// Gets the byte representation of the private key
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Signs a message with this private key
    pub fn sign(&self, message: &[u8]) -> Signature {
        let ed_sig = self.0.sign(message);
        Signature(ed_sig)
    }

    /// Converts the private key to a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// Creates a private key from a hex string
    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let bytes = hex::decode(hex_str)?;
        Self::from_bytes(&bytes)
    }
}

// Implement Serialize for Signature
impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

// Implement Deserialize for Signature
impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Signature::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

// Implement Serialize for PublicKey
impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

// Implement Deserialize for PublicKey
impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PublicKey::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({})", self.to_hex())
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.to_hex())
    }
}

impl fmt::Display for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[PRIVATE KEY]")
    }
}

impl fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PrivateKey([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn test_keypair() -> (PrivateKey, PublicKey) {
        let mut rng = StdRng::seed_from_u64(42);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        (sk, pk)
    }

    #[test]
    fn sign_verify_round_trip() {
        let (sk, pk) = test_keypair();
        let msg = b"hello wrfcoin";
        let sig = sk.sign(msg);
        assert!(pk.verify(msg, &sig));
    }

    #[test]
    fn wrong_key_rejects() {
        let (sk_a, _pk_a) = test_keypair();
        let mut rng = StdRng::seed_from_u64(99);
        let sk_b = PrivateKey::generate(&mut rng);
        let pk_b = PublicKey::from_private_key(&sk_b);

        let sig = sk_a.sign(b"message");
        assert!(!pk_b.verify(b"message", &sig));
    }

    #[test]
    fn tampered_message_rejects() {
        let (sk, pk) = test_keypair();
        let sig = sk.sign(b"original message");
        assert!(!pk.verify(b"tampered message", &sig));
    }

    #[test]
    fn verify_strict_rejects_malleated_signature_2043() {
        // #2043: verify()/verify_signature() use verify_strict, which rejects a
        // non-canonical S (S >= L). Adding the group order L to the S half of a
        // valid signature yields a different 64-byte encoding that the permissive
        // Verifier::verify would accept (same R, S ≡ S+L mod L) but strict
        // verification must reject — closing the txid-malleability surface (the
        // tx-id folds the signature in via calculate_hash while signing_hash
        // excludes it, so a malleated sig would otherwise mint an alternate txid
        // for the same logical transfer).
        let (sk, pk) = test_keypair();
        let msg = b"malleability guard";
        let sig = sk.sign(msg);
        assert!(pk.verify(msg, &sig), "honest signature must still verify");

        // ed25519 group order L, little-endian 32 bytes.
        const L_LE: [u8; 32] = [
            0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9,
            0xde, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x10,
        ];
        let mut bytes = hex::decode(sig.to_hex()).expect("signature hex decodes");
        assert_eq!(bytes.len(), 64);
        // S = bytes[32..64] += L (little-endian add with carry). Honest S < L <
        // 2^252, so S + L < 2^253 — no 32-byte overflow.
        let mut carry = 0u16;
        for i in 0..32 {
            let sum = bytes[32 + i] as u16 + L_LE[i] as u16 + carry;
            bytes[32 + i] = (sum & 0xff) as u8;
            carry = sum >> 8;
        }
        assert_eq!(carry, 0, "S + L must not overflow 32 bytes");

        // Robust to either rejection point: verify_signature returns false if the
        // non-canonical encoding is rejected at parse OR at strict verification.
        assert!(
            !pk.verify_signature(msg, &bytes),
            "malleated (S + L) signature must be rejected"
        );
    }

    #[test]
    fn tampered_signature_rejects() {
        let (sk, pk) = test_keypair();
        let sig = sk.sign(b"test message");
        let mut sig_bytes = sig.as_bytes();
        sig_bytes[0] ^= 0xff; // flip first byte
        let tampered = Signature::from_bytes(&sig_bytes).unwrap();
        assert!(!pk.verify(b"test message", &tampered));
    }

    #[test]
    fn empty_message_sign_verify() {
        let (sk, pk) = test_keypair();
        let sig = sk.sign(b"");
        assert!(pk.verify(b"", &sig));
    }

    #[test]
    fn deterministic_signatures() {
        let (sk, _pk) = test_keypair();
        let msg = b"determinism check";
        let sig1 = sk.sign(msg);
        let sig2 = sk.sign(msg);
        assert_eq!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[test]
    fn hex_round_trip() {
        let (sk, pk) = test_keypair();
        let sig = sk.sign(b"hex test");

        // PrivateKey hex round-trip
        let sk2 = PrivateKey::from_hex(&sk.to_hex()).unwrap();
        assert_eq!(sk.as_bytes(), sk2.as_bytes());  // secret-leak-ok: test-only fixture key, never a production output path

        // PublicKey hex round-trip
        let pk2 = PublicKey::from_hex(&pk.to_hex()).unwrap();
        assert_eq!(pk.as_bytes(), pk2.as_bytes());

        // Signature hex round-trip
        let sig2 = Signature::from_hex(&sig.to_hex()).unwrap();
        assert_eq!(sig.as_bytes(), sig2.as_bytes());
    }

    #[test]
    fn from_bytes_rejects_invalid_length() {
        assert!(Signature::from_bytes(&[0u8; 10]).is_err());
        assert!(PublicKey::from_bytes(&[0u8; 10]).is_err());
        assert!(PrivateKey::from_bytes(&[0u8; 10]).is_err());  // secret-leak-ok: test-only fixture key, never a production output path
    }

    #[test]
    fn serde_json_round_trip() {
        let (sk, pk) = test_keypair();
        let sig = sk.sign(b"serde test");

        // Signature serde round-trip
        let sig_json = serde_json::to_string(&sig).unwrap();
        let sig_back: Signature = serde_json::from_str(&sig_json).unwrap();
        assert_eq!(sig.as_bytes(), sig_back.as_bytes());

        // PublicKey serde round-trip
        let pk_json = serde_json::to_string(&pk).unwrap();
        let pk_back: PublicKey = serde_json::from_str(&pk_json).unwrap();
        assert_eq!(pk.as_bytes(), pk_back.as_bytes());
    }

    #[test]
    fn address_is_deterministic() {
        let (_sk, pk) = test_keypair();
        let addr1 = pk.to_address();
        let addr2 = pk.to_address();
        assert_eq!(addr1, addr2);
        assert!(addr1.starts_with("wrf"));
        // 20 bytes hex-encoded = 40 chars + "wrf" prefix = 43 chars
        assert_eq!(addr1.len(), 43);
    }

    // ── #1735 hardening batch 10a: verify_signature() bytes-path tests ──────────
    //
    // `PublicKey::verify_signature(msg, sig_bytes)` is the bytes-level API used
    // by external callers (block deserialization, P2P peer messages). All existing
    // tests use the typed `verify(msg, &Signature)` path. These tests exercise
    // the bytes path and pin its fail-safe behavior (wrong-length → false, no panic).

    /// `verify_signature` with a valid signature (as raw bytes) must return true.
    ///
    /// This exercises the full bytes → try_from → verify pipeline that the
    /// typed `verify()` path bypasses.
    #[test]
    fn verify_signature_valid_roundtrip() {
        let (sk, pk) = test_keypair();
        let msg = b"block header payload";
        let sig = sk.sign(msg);
        let sig_bytes = sig.as_bytes();
        assert!(
            pk.verify_signature(msg, &sig_bytes),
            "verify_signature with valid sig bytes must return true"
        );
    }

    /// `verify_signature` with a tampered message must return false.
    #[test]
    fn verify_signature_tampered_message_returns_false() {
        let (sk, pk) = test_keypair();
        let sig = sk.sign(b"original");
        let sig_bytes = sig.as_bytes();
        assert!(
            !pk.verify_signature(b"tampered", &sig_bytes),
            "verify_signature must return false for a tampered message"
        );
    }

    /// `verify_signature` with wrong-length bytes must return false without
    /// panicking.  The length guard (`signature_bytes.len() != SIGNATURE_LENGTH`)
    /// is the first thing checked; this test pins it cannot be removed.
    #[test]
    fn verify_signature_wrong_length_bytes_returns_false_not_panic() {
        let (_sk, pk) = test_keypair();
        let msg = b"test";
        // Too short
        assert!(
            !pk.verify_signature(msg, &[0u8; 10]),
            "verify_signature with 10-byte slice must return false"
        );
        // Too long
        assert!(
            !pk.verify_signature(msg, &[0u8; 65]),
            "verify_signature with 65-byte slice must return false"
        );
        // Zero length
        assert!(
            !pk.verify_signature(msg, &[]),
            "verify_signature with empty slice must return false"
        );
    }

    // ── #1735 hardening batch 10b: from_bytes() length guard completeness ───────
    //
    // The existing `from_bytes_rejects_invalid_length` test only covers the
    // too-short case ([0u8; 10]).  These tests cover the symmetric too-long
    // cases (one byte over) and the zero-length edge, plus a valid-length but
    // invalid-curve-point input for PublicKey.

    /// `from_bytes` on too-long slices must return Err — length guard must
    /// catch both ends (< and >), not just the short side.
    #[test]
    fn from_bytes_rejects_too_long_slice() {
        // SIGNATURE_LENGTH = 64; 65 bytes is one over
        assert!(
            Signature::from_bytes(&[0u8; 65]).is_err(),
            "Signature::from_bytes with 65 bytes must return Err"
        );
        // PUBLIC_KEY_LENGTH = 32; 33 bytes is one over
        assert!(
            PublicKey::from_bytes(&[0u8; 33]).is_err(),
            "PublicKey::from_bytes with 33 bytes must return Err"
        );
        // SECRET_KEY_LENGTH = 32; 33 bytes is one over
        assert!(
            PrivateKey::from_bytes(&[0u8; 33]).is_err(),
            "PrivateKey::from_bytes with 33 bytes must return Err"
        );
    }

    /// `Signature::from_bytes` with a valid-length (64-byte) all-zero slice
    /// must succeed (the type is a pure byte container; construction does NOT
    /// validate the scalar or group element).  The resulting signature must NOT
    /// verify against any message — validation is deferred to `verify()`.
    ///
    /// This pins the two-phase parse-then-verify contract: callers that only
    /// call `from_bytes` without subsequently calling `verify()` cannot assume
    /// the result is cryptographically sound.
    #[test]
    fn signature_zero_bytes_parses_but_does_not_verify() {
        let (_sk, pk) = test_keypair();
        let zero_sig = Signature::from_bytes(&[0u8; 64])
            .expect("Signature::from_bytes with valid-length all-zero bytes must succeed");
        // The zero signature is not a valid signature for any message.
        assert!(
            !pk.verify(b"any message", &zero_sig),
            "All-zero signature must not verify against any real public key"
        );
    }
}
