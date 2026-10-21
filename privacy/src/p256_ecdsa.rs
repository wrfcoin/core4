//! P-256 (secp256r1) ECDSA verification for WebAuthn passkeys (#2169).
//!
//! # Why this lives in the privacy crate
//!
//! The consumer is `wrfcoin-shared-protocol::accounts::passkey`, but that
//! crate's direct runtime dependencies are budgeted
//! (`scripts/check-dep-budget.sh`, 30) and sit exactly at the budget. This
//! crate is already in its graph and is where curve arithmetic lives, so the
//! `p256` dependency is taken here and the verifier is re-exported. Nothing in
//! this module knows about WebAuthn, transactions or addresses: it is the bare
//! curve primitive, and the WebAuthn envelope (client data, authenticator
//! data, RP ID, flags, counter) is checked by the caller.
//!
//! # What it is NOT
//!
//! This is a **separate curve** from `k256` (secp256k1, used for EVM
//! compatibility) and from the chain's native ed25519. A P-256 key never
//! verifies an ed25519 preimage and vice versa; the two never share a type.
//!
//! # Malleability
//!
//! ECDSA signatures are malleable: `(r, s)` and `(r, n - s)` both verify.
//! [`verify_sha256`] is the pure curve check and accepts both, which is what
//! the Wycheproof harness (`shared-protocol/tests/passkey_webauthn_2169.rs`)
//! exercises. [`verify_sha256_low_s`] additionally rejects the high-`s` twin
//! and is the form consensus code must use, so a relayer cannot re-encode a
//! signed authorization into a second distinct transaction. A WebAuthn client
//! normalises `s` before submitting (every authenticator returns an `(r, s)`
//! that is equally valid after normalisation); an un-normalised assertion is
//! refused with [`P256Error::HighS`], never silently accepted.

use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

/// Why a P-256 signature was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P256Error {
    /// The public key is not a valid SEC1 encoding of a point on P-256.
    InvalidPublicKey,
    /// The signature bytes are not a valid DER or fixed-width `(r, s)` pair
    /// with both scalars in `[1, n)`.
    InvalidSignatureEncoding,
    /// The signature is in high-`s` form (malleable twin). Clients must
    /// normalise `s <= n/2` before submitting.
    HighS,
    /// The signature does not verify over the message.
    SignatureInvalid,
}

impl std::fmt::Display for P256Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::InvalidPublicKey => "invalid P-256 public key encoding",
            Self::InvalidSignatureEncoding => "invalid P-256 signature encoding",
            Self::HighS => "P-256 signature is not low-s normalised",
            Self::SignatureInvalid => "P-256 signature does not verify",
        };
        f.write_str(text)
    }
}

impl std::error::Error for P256Error {}

/// Length of an uncompressed SEC1 P-256 public key (`0x04 || X || Y`).
pub const SEC1_UNCOMPRESSED_LEN: usize = 65;
/// Length of a raw `X || Y` P-256 public key, as WebAuthn COSE keys carry it.
pub const RAW_XY_LEN: usize = 64;
/// Length of a fixed-width `r || s` P-256 signature.
pub const FIXED_SIGNATURE_LEN: usize = 64;

/// A P-256 verifying key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P256PublicKey(VerifyingKey);

impl P256PublicKey {
    /// Parse a SEC1-encoded point (compressed `0x02/0x03 || X` or
    /// uncompressed `0x04 || X || Y`). The identity and off-curve points are
    /// rejected.
    pub fn from_sec1_bytes(bytes: &[u8]) -> Result<Self, P256Error> {
        VerifyingKey::from_sec1_bytes(bytes)
            .map(Self)
            .map_err(|_| P256Error::InvalidPublicKey)
    }

    /// Parse a raw `X || Y` affine coordinate pair (the form a WebAuthn COSE
    /// EC2 key carries after the `-2`/`-3` labels are stripped).
    pub fn from_raw_xy(xy: &[u8; RAW_XY_LEN]) -> Result<Self, P256Error> {
        let mut sec1 = [0u8; SEC1_UNCOMPRESSED_LEN];
        sec1[0] = 0x04;
        sec1[1..].copy_from_slice(xy);
        Self::from_sec1_bytes(&sec1)
    }

    /// The uncompressed SEC1 encoding (`0x04 || X || Y`, 65 bytes).
    pub fn to_sec1_uncompressed(&self) -> [u8; SEC1_UNCOMPRESSED_LEN] {
        let point = self.0.to_encoded_point(false);
        let mut out = [0u8; SEC1_UNCOMPRESSED_LEN];
        out.copy_from_slice(point.as_bytes());
        out
    }

    /// The raw `X || Y` coordinates (64 bytes).
    pub fn to_raw_xy(&self) -> [u8; RAW_XY_LEN] {
        let sec1 = self.to_sec1_uncompressed();
        let mut out = [0u8; RAW_XY_LEN];
        out.copy_from_slice(&sec1[1..]);
        out
    }
}

/// A P-256 ECDSA signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P256Signature(Signature);

impl P256Signature {
    /// Parse an ASN.1 DER `ECDSA-Sig-Value` (the form WebAuthn authenticators
    /// return).
    pub fn from_der(bytes: &[u8]) -> Result<Self, P256Error> {
        Signature::from_der(bytes)
            .map(Self)
            .map_err(|_| P256Error::InvalidSignatureEncoding)
    }

    /// Parse a fixed-width `r || s` pair (64 bytes, IEEE P1363 form).
    pub fn from_fixed_bytes(bytes: &[u8]) -> Result<Self, P256Error> {
        if bytes.len() != FIXED_SIGNATURE_LEN {
            return Err(P256Error::InvalidSignatureEncoding);
        }
        Signature::from_slice(bytes)
            .map(Self)
            .map_err(|_| P256Error::InvalidSignatureEncoding)
    }

    /// Fixed-width `r || s` encoding.
    pub fn to_fixed_bytes(&self) -> [u8; FIXED_SIGNATURE_LEN] {
        let mut out = [0u8; FIXED_SIGNATURE_LEN];
        out.copy_from_slice(&self.0.to_bytes());
        out
    }

    /// DER encoding.
    pub fn to_der_bytes(&self) -> Vec<u8> {
        self.0.to_der().as_bytes().to_vec()
    }

    /// Whether `s <= n/2` (the non-malleable form).
    pub fn is_low_s(&self) -> bool {
        self.0.normalize_s().is_none()
    }

    /// The low-`s` form of this signature (itself, if already low-`s`).
    pub fn normalized(&self) -> Self {
        Self(self.0.normalize_s().unwrap_or(self.0))
    }

    /// The high-`s` twin `(r, n - s)`; used by tests to prove the low-`s`
    /// rule bites. Returns `None` when `s == n/2` exactly (its twin is itself).
    pub fn malleable_twin(&self) -> Option<Self> {
        let r = self.0.r();
        let s = self.0.s();
        let neg_s = -*s;
        let twin = Signature::from_scalars(r.to_bytes(), neg_s.to_bytes()).ok()?;
        (twin != self.0).then_some(Self(twin))
    }
}

/// A P-256 signing key. The chain never signs P-256 — this exists for test
/// fixtures, golden vectors and as the reference a wallet SDK conforms to
/// (RFC 6979 deterministic nonces, output always low-`s`).
///
/// Deliberately NOT `Clone`: every copy of a signing scalar is another
/// unzeroized secret in ordinary memory (`scripts/check_secret_leak.py`), and
/// nothing here needs one. `Debug` is implemented by hand and redacts.
pub struct P256SigningKey(SigningKey);

impl std::fmt::Debug for P256SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("P256SigningKey(<redacted>)")
    }
}

impl P256SigningKey {
    /// Build from a 32-byte big-endian scalar in `[1, n)`.
    pub fn from_bytes(secret: &[u8; 32]) -> Result<Self, P256Error> {
        SigningKey::from_slice(secret)
            .map(Self)
            .map_err(|_| P256Error::InvalidPublicKey)
    }

    /// The verifying key.
    pub fn public_key(&self) -> P256PublicKey {
        P256PublicKey(*self.0.verifying_key())
    }

    /// Sign `SHA-256(message)`, returning the low-`s` form.
    pub fn sign_sha256_low_s(&self, message: &[u8]) -> P256Signature {
        let sig: Signature = self.0.sign(message);
        P256Signature(sig).normalized()
    }
}

/// Pure ECDSA-P256-SHA256 verification. Accepts both malleable forms; this
/// is the Wycheproof-conformant primitive, not the consensus rule.
pub fn verify_sha256(key: &P256PublicKey, message: &[u8], signature: &P256Signature) -> bool {
    key.0.verify(message, &signature.0).is_ok()
}

/// ECDSA-P256-SHA256 verification with the low-`s` rule. The form consensus
/// code uses: a high-`s` signature is refused with [`P256Error::HighS`] before
/// the curve check runs.
pub fn verify_sha256_low_s(
    key: &P256PublicKey,
    message: &[u8],
    signature: &P256Signature,
) -> Result<(), P256Error> {
    if !signature.is_low_s() {
        return Err(P256Error::HighS);
    }
    if verify_sha256(key, message, signature) {
        Ok(())
    } else {
        Err(P256Error::SignatureInvalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_key() -> P256SigningKey {
        // A fixed, non-secret test scalar: deterministic across runs and
        // machines (RFC 6979 signing), so every assertion below is stable.
        let secret = [
            0x1f, 0x2e, 0x3d, 0x4c, 0x5b, 0x6a, 0x79, 0x88, 0x97, 0xa6, 0xb5, 0xc4, 0xd3, 0xe2,
            0xf1, 0x00, 0x0f, 0x1e, 0x2d, 0x3c, 0x4b, 0x5a, 0x69, 0x78, 0x87, 0x96, 0xa5, 0xb4,
            0xc3, 0xd2, 0xe1, 0xf0,
        ];
        P256SigningKey::from_bytes(&secret).expect("scalar in range")
    }

    #[test]
    fn sign_verify_round_trip_in_both_encodings() {
        let sk = fixed_key();
        let pk = sk.public_key();
        let msg = b"wrfcoin p256 round trip";
        let low = sk.sign_sha256_low_s(msg);
        assert!(low.is_low_s());
        assert!(verify_sha256(&pk, msg, &low));
        assert_eq!(verify_sha256_low_s(&pk, msg, &low), Ok(()));

        let der = P256Signature::from_der(&low.to_der_bytes()).unwrap();
        let fixed = P256Signature::from_fixed_bytes(&low.to_fixed_bytes()).unwrap();
        assert_eq!(der, low);
        assert_eq!(fixed, low);

        // Raw X||Y and SEC1 name the same key.
        let raw = P256PublicKey::from_raw_xy(&pk.to_raw_xy()).unwrap();
        assert_eq!(raw, pk);
        assert_eq!(raw.to_sec1_uncompressed()[0], 0x04);
    }

    #[test]
    fn a_tampered_message_fails() {
        let sk = fixed_key();
        let pk = P256PublicKey::from_raw_xy(&sk.public_key().to_raw_xy()).unwrap();
        let sig = sk.sign_sha256_low_s(b"message A");
        assert!(!verify_sha256(&pk, b"message B", &sig));
        assert_eq!(
            verify_sha256_low_s(&pk, b"message B", &sig),
            Err(P256Error::SignatureInvalid)
        );
    }

    #[test]
    fn the_high_s_twin_verifies_on_the_curve_but_is_refused_by_the_low_s_rule() {
        let sk = fixed_key();
        let pk = sk.public_key();
        let msg = b"malleability";
        let low = sk.sign_sha256_low_s(msg);
        let high = low.malleable_twin().expect("s != n/2");
        assert!(!high.is_low_s());
        assert_ne!(high.to_fixed_bytes(), low.to_fixed_bytes());
        // Pure ECDSA accepts both forms — that IS the malleability.
        assert!(verify_sha256(&pk, msg, &high));
        // The consensus form does not.
        assert_eq!(verify_sha256_low_s(&pk, msg, &high), Err(P256Error::HighS));
        assert_eq!(high.normalized(), low);
    }

    #[test]
    fn malformed_inputs_are_typed_errors() {
        assert_eq!(
            P256PublicKey::from_sec1_bytes(&[0x04; 65]).unwrap_err(),
            P256Error::InvalidPublicKey
        );
        assert_eq!(
            P256PublicKey::from_sec1_bytes(&[]).unwrap_err(),
            P256Error::InvalidPublicKey
        );
        assert_eq!(
            P256Signature::from_der(&[0x30, 0x00]).unwrap_err(),
            P256Error::InvalidSignatureEncoding
        );
        assert_eq!(
            P256Signature::from_fixed_bytes(&[0u8; 64]).unwrap_err(),
            P256Error::InvalidSignatureEncoding
        );
        assert_eq!(
            P256Signature::from_fixed_bytes(&[1u8; 63]).unwrap_err(),
            P256Error::InvalidSignatureEncoding
        );
    }
}
