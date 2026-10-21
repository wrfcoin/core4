//! ed25519 signature-malleability rejection (#2127).
//!
//! # The malleability surface
//!
//! An ed25519 signature is `(R, S)` with `S` a scalar mod the group order `L`.
//! A permissive verifier accepts any `S` and reduces it, so `S' = S + L`
//! verifies just as well as `S` — a *different* 32-byte signature over the same
//! message under the same key. That is signature malleability, and in WRFCoin
//! it is worse than cosmetic: the transaction id (`calculate_hash`) folds in the
//! input signature while `signing_hash` excludes it, so a malleated signature
//! yields a *different* tx-id for the same logical transfer (txid-malleability).
//!
//! WRFCoin's wrapper verifies with `verify_strict` (#2043), which rejects
//! non-canonical `S` (`S >= L`) and non-canonical `R`. These tests prove the
//! wrapper — not the upstream crate — enforces that.
//!
//! The counterpart ECDSA case in the story (low-`s` normalisation) has no
//! project wrapper in `shared-protocol` to test — k256 is used directly by
//! other crates — so it is recorded in [`ECDSA_LOW_S_NO_WRAPPER`] rather than
//! tested against upstream, consistent with the #2126 harness decision.

use ed25519_dalek::{Signer, SigningKey};
use wrfcoin_shared_protocol::crypto::signature::{PublicKey, Signature};

/// ed25519 group order L = 2^252 + 27742317777372353535851937790883648493,
/// little-endian.
const GROUP_ORDER_L_LE: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

/// No project wrapper exists for k256/ECDSA in `shared-protocol`, so the story's
/// low-`s` ECDSA case cannot be tested here without exercising the upstream
/// crate directly. Recorded, consistent with #2126's `K256_NO_PROJECT_WRAPPER`.
pub const ECDSA_LOW_S_NO_WRAPPER: &str = "core4#2127: no k256/ECDSA wrapper in shared-protocol; low-s normalisation must be tested \
     wherever a project wrapper lands (see also #2126)";

/// Deterministic keypair + a valid signature over `msg`, using the upstream
/// signer directly (the *signing* side is not what is under test — verification
/// is).
fn valid_signature(msg: &[u8]) -> (PublicKey, [u8; 64]) {
    let sk = SigningKey::from_bytes(&[3u8; 32]);
    let sig = sk.sign(msg).to_bytes();
    let pk = PublicKey::from_bytes(sk.verifying_key().as_bytes()).unwrap();
    (pk, sig)
}

/// Add `L` to the scalar half of a signature, little-endian, producing a
/// non-canonical `S' = S + L`.
fn add_group_order_to_s(sig: &[u8; 64]) -> [u8; 64] {
    let mut out = *sig;
    let mut carry = 0u16;
    for i in 0..32 {
        let sum = u16::from(out[32 + i]) + u16::from(GROUP_ORDER_L_LE[i]) + carry;
        out[32 + i] = (sum & 0xff) as u8;
        carry = sum >> 8;
    }
    out
}

#[test]
fn valid_signature_verifies() {
    // Anchor: if this fails, every rejection below is vacuous.
    let msg = b"transfer";
    let (pk, sig) = valid_signature(msg);
    assert!(pk.verify_signature(msg, &sig));
}

#[test]
fn high_s_variant_is_rejected() {
    // The core malleability case: S' = S + L is a distinct, valid-looking
    // signature that a non-strict verifier accepts. verify_strict must reject.
    let msg = b"transfer";
    let (pk, sig) = valid_signature(msg);
    let malleated = add_group_order_to_s(&sig);

    assert_ne!(sig, malleated, "S+L must be a different 32-byte signature");
    assert!(
        pk.verify_signature(msg, &sig),
        "the canonical signature still verifies"
    );
    assert!(
        !pk.verify_signature(msg, &malleated),
        "a high-S (S+L) signature over a valid message must be REJECTED, not accepted"
    );
}

#[test]
fn high_s_variant_is_rejected_through_the_typed_api_too() {
    // The typed `PublicKey::verify` path must reject it as well — a bug can live
    // in either entry point.
    let msg = b"transfer";
    let (pk, sig) = valid_signature(msg);
    let malleated = add_group_order_to_s(&sig);

    if let Ok(parsed) = Signature::from_bytes(&malleated) {
        assert!(
            !pk.verify(msg, &parsed),
            "typed verify must also reject the malleated signature"
        );
    }
    // If `from_bytes` itself rejects it, that is an even stronger rejection —
    // either outcome is acceptable, verification-accepts is not.
}

#[test]
fn non_canonical_s_all_max_is_rejected() {
    // S set to all-ones is far above L and must be rejected.
    let msg = b"transfer";
    let (pk, sig) = valid_signature(msg);
    let mut bad = sig;
    bad[32..].copy_from_slice(&[0xffu8; 32]);
    assert!(
        !pk.verify_signature(msg, &bad),
        "an all-ones S encoding is non-canonical and must be rejected"
    );
}

#[test]
fn tampered_r_is_rejected() {
    let msg = b"transfer";
    let (pk, sig) = valid_signature(msg);
    let mut bad = sig;
    bad[0] ^= 0x01;
    assert!(
        !pk.verify_signature(msg, &bad),
        "a signature with a flipped R byte must be rejected"
    );
}

#[test]
fn wrong_length_signature_is_rejected_not_panicked() {
    let msg = b"transfer";
    let (pk, _) = valid_signature(msg);
    assert!(
        !pk.verify_signature(msg, &[0u8; 63]),
        "a 63-byte signature must be rejected, never panic"
    );
    assert!(!pk.verify_signature(msg, &[]));
}

#[test]
fn ecdsa_low_s_scope_is_recorded() {
    assert!(ECDSA_LOW_S_NO_WRAPPER.contains("2127"));
}

// ---------------------------------------------------------------------------
// Degenerate public keys, driven through the production verifier
// ---------------------------------------------------------------------------
//
// Review of #2237: the identity tests "never pass identity into the
// verification or public-input path whose contract requires identity to be
// rejected." On the ed25519 side that path is
// `PublicKey::verify_signature` — the wrapper every caller uses — and the
// contract comes from `verify_strict` (#2043), which rejects small-order public
// keys where the permissive `verify` accepts them.
//
// So these drive the degenerate KEY through the real verifier, rather than
// constructing it and observing that it is distinguishable.

/// The compressed Edwards identity (`y = 1`): the all-zero-ish small-order key.
const EDWARDS_IDENTITY_PUBKEY: [u8; 32] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// A genuine non-identity Edwards25519 point of order exactly 8.
///
/// Same fixture as `privacy/src/tests/degenerate_inputs.rs`, where its order is
/// proved rather than asserted. It is repeated here rather than shared because
/// `privacy` does not depend on `shared-protocol` in that direction.
const EDWARDS_ORDER_8_PUBKEY: [u8; 32] = [
    0xc7, 0x17, 0x6a, 0x70, 0x3d, 0x4d, 0xd8, 0x4f, 0xba, 0x3c, 0x0b, 0x76, 0x0d, 0x10, 0x67, 0x0f,
    0x2a, 0x20, 0x53, 0xfa, 0x2c, 0x39, 0xcc, 0xc6, 0x4e, 0xc7, 0xfd, 0x77, 0x92, 0xac, 0x03, 0x7a,
];

/// The identity as a public key must never verify a signature.
///
/// Breaks if: `PublicKey::verify_signature` stops routing through
/// `verify_strict` — the permissive `ed25519_dalek::verify` accepts small-order
/// keys, which is the #2043 regression this guards.
#[test]
fn identity_public_key_verifies_nothing() {
    let (_, sig) = valid_signature(b"msg");
    // Asserted, not `let ... else { return }`: an early return would turn this
    // test into a silent no-op the day `from_bytes` starts rejecting these keys,
    // which is the exact defect class #2237's review is about. Verified today:
    // the identity DOES parse, so the assertion below really runs.
    let pk = PublicKey::from_bytes(&EDWARDS_IDENTITY_PUBKEY).expect(
        "the identity currently parses as a public key — if this now fails, \
         rejection moved to parse time, which is stronger: change this test \
         deliberately to assert that instead of deleting it",
    );
    assert!(
        !pk.verify_signature(b"msg", &sig),
        "the identity public key must not verify any signature"
    );
    // And it must not verify a signature that is genuinely valid under a real
    // key either — i.e. it is not simply that this particular signature is bad.
    let (real_pk, real_sig) = valid_signature(b"other");
    assert!(
        real_pk.verify_signature(b"other", &real_sig),
        "control: the real key must verify its own signature"
    );
    assert!(
        !pk.verify_signature(b"other", &real_sig),
        "the identity public key must not verify another key's valid signature"
    );
}

/// A non-identity small-order public key must never verify a signature.
///
/// This is the case the identity does not cover: an order-8 point is a
/// *different* degenerate class, and a check written as `key != identity` would
/// pass the test above while failing this one.
///
/// Breaks if: `PublicKey::verify_signature` stops routing through
/// `verify_strict`.
#[test]
fn small_order_public_key_verifies_nothing() {
    let (real_pk, real_sig) = valid_signature(b"msg");
    assert!(
        real_pk.verify_signature(b"msg", &real_sig),
        "control: the real key must verify its own signature"
    );

    let pk = PublicKey::from_bytes(&EDWARDS_ORDER_8_PUBKEY).expect(
        "the order-8 point currently parses as a public key — if this now fails, \
         rejection moved to parse time: change this test deliberately",
    );
    assert!(
        !pk.verify_signature(b"msg", &real_sig),
        "an order-8 small-subgroup public key must not verify any signature"
    );
}
