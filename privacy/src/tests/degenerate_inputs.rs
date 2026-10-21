//! Degenerate curve-input rejection (#2127).
//!
//! Negative tests proving the privacy primitives reject the degenerate inputs
//! that break naive implementations: identity / point-at-infinity, small-order
//! and non-prime-order subgroup points, off-curve encodings, out-of-range
//! scalars, and — for the double-spend surface — key-image reuse.
//!
//! # Why Ristretto changes the shape of these tests
//!
//! Every point here is a **Ristretto** point, and that is not incidental.
//! Ristretto is a quotient construction over Edwards25519 that eliminates the
//! cofactor: every valid encoding maps to a distinct prime-order group element,
//! and the classic small-order / non-prime-order-subgroup and
//! equal-but-different-encoding attacks are ruled out *by the encoding*, not by
//! a check the caller must remember. So for several primitives the assertion is
//! "the degenerate encoding fails to decompress at the trust boundary," which is
//! the Ristretto guarantee doing its job — and these tests fail loudly if a
//! future refactor swaps Ristretto for raw Edwards and reintroduces the surface.
//!
//! Each test asserts the **specific** `PrivacyError` variant where the API
//! exposes one. "Rejected for the wrong reason" is a real failure mode: a check
//! that happens to reject today's input via an unrelated code path stops
//! rejecting the moment that path changes.

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;
use curve25519_dalek::edwards::CompressedEdwardsY;
use curve25519_dalek::traits::IsIdentity;
use curve25519_dalek::ristretto::CompressedRistretto;
use curve25519_dalek::scalar::Scalar;

use crate::error::PrivacyError;
use crate::key_image_registry::KeyImageRegistry;
use crate::pedersen::PedersenCommitment;
use crate::ring_signatures::{KeyImage, MLSAGSignature, RingMember};

/// The 32-byte all-zero encoding. Under Ristretto this is the canonical
/// encoding of the identity element.
const ALL_ZERO: [u8; 32] = [0u8; 32];
/// The 32-byte all-ones encoding — not a canonical Ristretto point.
const ALL_MAX: [u8; 32] = [0xffu8; 32];

/// A genuine **non-identity** Edwards25519 point of order exactly 8.
///
/// # What was wrong before, and why it matters
///
/// This constant previously held `01` followed by 31 zero bytes. In compressed
/// Edwards form that is `y = 1, sign = 0` — **the identity element**, not a
/// small-subgroup point. Every test naming it "order 8" was therefore exercising
/// the identity class under a subgroup label. Review of #2237 caught it, and it
/// is the failure this fixture exists to prevent: a value that is nearly the
/// right class is worse than an obviously wrong one, because it passes.
///
/// # Derivation
///
/// Edwards25519 has cofactor 8, so its torsion subgroup has exactly 8 elements:
/// one of order 1 (the identity), one of order 2, two of order 4, and **four of
/// order 8**. This is one of the four, in the standard little-endian compressed
/// encoding used by RFC 8032:
///
/// ```text
/// c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a
/// ```
///
/// # Why it has the order claimed — checked, not asserted
///
/// [`the_torsion_fixture_really_has_order_8`] proves it in-tree rather than
/// asking the reader to trust the name:
///
/// - it decompresses as an Edwards point at all,
/// - `is_small_order()` is true (it lies in the 8-element torsion subgroup),
/// - it is **not** the identity — the property the old fixture failed,
/// - `8 * P` **is** the identity, so its order divides 8,
/// - `4 * P` is **not** the identity, so its order does not divide 4.
///
/// Together those pin the order at exactly 8. If a future edit substitutes a
/// different constant, that test fails rather than the label quietly going stale.
const EDWARDS_ORDER_8_TORSION: [u8; 32] = [
    0xc7, 0x17, 0x6a, 0x70, 0x3d, 0x4d, 0xd8, 0x4f, 0xba, 0x3c, 0x0b, 0x76, 0x0d, 0x10, 0x67, 0x0f,
    0x2a, 0x20, 0x53, 0xfa, 0x2c, 0x39, 0xcc, 0xc6, 0x4e, 0xc7, 0xfd, 0x77, 0x92, 0xac, 0x03, 0x7a,
];

/// The compressed **Edwards** identity (`y = 1`). Kept as a separate, honestly
/// named constant so the class it belongs to is legible: it is what
/// `EDWARDS_ORDER_8_TORSION` used to hold by mistake.
const EDWARDS_IDENTITY: [u8; 32] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn valid_point_bytes(seed: u64) -> [u8; 32] {
    let scalar = Scalar::from(seed.max(1));
    (&scalar * RISTRETTO_BASEPOINT_TABLE).compress().to_bytes()
}

// ---------------------------------------------------------------------------
// The fixture validates itself
// ---------------------------------------------------------------------------

/// Prove `EDWARDS_ORDER_8_TORSION` is what its name says.
///
/// Breaks if: the constant is edited to any point that is not of order exactly
/// 8 — including the identity, which is what it previously held.
#[test]
fn the_torsion_fixture_really_has_order_8() {
    let p = CompressedEdwardsY(EDWARDS_ORDER_8_TORSION)
        .decompress()
        .expect("the fixture must be a valid Edwards point at all");

    assert!(
        p.is_small_order(),
        "fixture must lie in the 8-element torsion subgroup"
    );
    // The property the old fixture failed. Without this line the constant could
    // silently revert to the identity encoding and every test naming it
    // "order 8" would keep passing.
    assert!(
        !p.is_identity(),
        "fixture must NOT be the identity — that is the class it is meant to \
         be distinguished from"
    );
    // Order divides 8 ...
    assert!(
        p.mul_by_cofactor().is_identity(),
        "8 * P must be the identity"
    );
    // ... and does not divide 4, so it is exactly 8.
    let four_p = p + p + p + p;
    assert!(
        !four_p.is_identity(),
        "4 * P must not be the identity, or the order is 4 or less"
    );
}

/// The identity constant is the identity — so the two fixtures cannot be
/// confused for one another again.
///
/// Breaks if: `EDWARDS_IDENTITY` stops being the identity, or someone sets the
/// two constants equal.
#[test]
fn the_identity_fixture_is_the_identity_and_differs_from_the_torsion_point() {
    let id = CompressedEdwardsY(EDWARDS_IDENTITY)
        .decompress()
        .expect("y = 1 is a valid Edwards encoding");
    assert!(id.is_identity(), "EDWARDS_IDENTITY must be the identity");
    assert_ne!(
        EDWARDS_IDENTITY, EDWARDS_ORDER_8_TORSION,
        "the identity and the torsion fixture must be distinct values"
    );
}

// ---------------------------------------------------------------------------
// Off-curve / non-canonical encodings must fail to decompress
// ---------------------------------------------------------------------------

#[test]
fn ristretto_rejects_all_max_encoding() {
    // 0xff..ff is not a canonical Ristretto encoding; decompression must fail.
    assert!(
        CompressedRistretto(ALL_MAX).decompress().is_none(),
        "all-ones must not decode to a Ristretto point"
    );
}

#[test]
fn ristretto_rejects_order_8_torsion_encoding() {
    // The small-order point a raw-Edwards impl would accept. Ristretto rejects
    // it at the encoding layer — this is the subgroup-attack surface being
    // closed by construction.
    assert!(
        CompressedRistretto(EDWARDS_ORDER_8_TORSION)
            .decompress()
            .is_none(),
        "an order-8 torsion encoding must not decode as Ristretto"
    );
}

#[test]
fn ristretto_identity_is_the_only_all_zero_and_is_distinguishable() {
    // All-zero *is* the identity under Ristretto (unlike raw Edwards, where the
    // identity has cofactor-many encodings). Confirm it decodes to the identity
    // and that the identity is not equal to a real generator point — so a check
    // "is this the identity" is meaningful.
    let identity = CompressedRistretto(ALL_ZERO)
        .decompress()
        .expect("all-zero is the canonical identity encoding");
    let g = CompressedRistretto(valid_point_bytes(1))
        .decompress()
        .expect("generator multiple decodes");
    assert_ne!(identity.compress().to_bytes(), g.compress().to_bytes());
}

// ---------------------------------------------------------------------------
// Pedersen commitments
// ---------------------------------------------------------------------------

#[test]
fn pedersen_from_bytes_rejects_wrong_length() {
    // A too-short buffer must be a typed InvalidCommitment, never a panic.
    match PedersenCommitment::from_bytes(&[0u8; 31]) {
        Err(PrivacyError::InvalidCommitment(_)) => {},
        other => panic!("expected InvalidCommitment for short input, got {other:?}"),
    }
}

#[test]
fn pedersen_from_bytes_rejects_off_curve_point() {
    match PedersenCommitment::from_bytes(&ALL_MAX) {
        Err(PrivacyError::InvalidCommitment(_)) => {},
        other => panic!("expected InvalidCommitment for off-curve point, got {other:?}"),
    }
}

#[test]
fn pedersen_from_bytes_rejects_order_8_torsion_point() {
    match PedersenCommitment::from_bytes(&EDWARDS_ORDER_8_TORSION) {
        Err(PrivacyError::InvalidCommitment(_)) => {},
        other => panic!("expected InvalidCommitment for torsion point, got {other:?}"),
    }
}

/// The identity must not open to **any** non-zero value, not just one.
///
/// Review of #2237: the previous version checked a single arbitrary value
/// (1 000) under a single blinding, which is a spot check, not the property.
///
/// Breaks if: `PedersenCommitment::verify` stops binding the value — e.g. an
/// early-return that treats the identity as trivially valid.
#[test]
fn pedersen_identity_opens_to_no_nonzero_value() {
    let identity =
        PedersenCommitment::from_bytes(&ALL_ZERO).expect("identity is a valid Ristretto encoding");
    let (_, blinding) = PedersenCommitment::commit(0);

    for value in [1u64, 2, 7, 1_000, u64::MAX / 2, u64::MAX] {
        assert!(
            !identity.verify(value, &blinding),
            "the identity must not verify as a commitment to {value}"
        );
    }
}

/// The converse, so the test above cannot pass by rejecting everything: the
/// identity IS the commitment to zero under a zero blinding, because
/// `0*G + 0*H` is the identity. Pinning it states what the identity legitimately
/// means rather than leaving it as "the thing we reject".
///
/// Breaks if: `verify` stops accepting a genuine opening.
#[test]
fn pedersen_identity_is_exactly_the_zero_commitment() {
    let identity =
        PedersenCommitment::from_bytes(&ALL_ZERO).expect("identity is a valid Ristretto encoding");
    assert!(
        identity.verify(0, &Scalar::ZERO),
        "the identity is 0*G + 0*H and must open to (value 0, blinding 0)"
    );
}

// ---------------------------------------------------------------------------
// Scalars: all-zero and all-max
// ---------------------------------------------------------------------------

#[test]
fn all_zero_scalar_yields_identity_public_key() {
    // x = 0 gives P = 0*G = identity. A key built from it must be recognisably
    // the identity, so downstream identity checks can catch it.
    let zero = Scalar::from_bytes_mod_order(ALL_ZERO);
    assert_eq!(zero, Scalar::ZERO);
    let pk = (&zero * RISTRETTO_BASEPOINT_TABLE).compress().to_bytes();
    assert_eq!(pk, ALL_ZERO, "0*G must be the identity encoding");
}

#[test]
fn all_max_scalar_is_reduced_mod_order_not_rejected_silently() {
    // 0xff..ff exceeds the group order. `from_bytes_mod_order` reduces it — the
    // hazard is assuming it is used verbatim. Confirm the reduction actually
    // happens (the reduced scalar differs from the raw bytes), so no code path
    // can treat an out-of-range scalar as in-range.
    let reduced = Scalar::from_bytes_mod_order(ALL_MAX);
    assert_ne!(
        reduced.to_bytes(),
        ALL_MAX,
        "an out-of-range scalar must be reduced, not passed through"
    );
    // And the reduced form is canonical: reducing it again is a no-op.
    assert_eq!(
        Scalar::from_bytes_mod_order(reduced.to_bytes()).to_bytes(),
        reduced.to_bytes()
    );
}

// ---------------------------------------------------------------------------
// Ring signatures: degenerate ring members
// ---------------------------------------------------------------------------

/// A message and a well-formed signature over a `MIN_RING_SIZE` ring.
fn signed_fixture() -> ([u8; 32], MLSAGSignature) {
    // `sign` inserts the signer itself, so it needs MIN_RING_SIZE - 1 = 6 decoys.
    let signer = [7u8; 32];
    let decoys: Vec<RingMember> = (0..6)
        .map(|i| RingMember {
            public_key: valid_point_bytes(100 + i),
        })
        .collect();
    let sig = MLSAGSignature::sign(b"msg", &signer, &decoys)
        .expect("signing a well-formed ring succeeds");
    (signer, sig)
}

#[test]
fn ring_signature_verifies_when_well_formed() {
    // Control: the negative tests below poison this fixture, so it must verify
    // clean or those tests would pass for the wrong reason.
    let (_, sig) = signed_fixture();
    assert!(sig.verify(b"msg").expect("verification runs"));
}

#[test]
fn ring_signature_rejects_off_curve_member_key() {
    // A ring member whose public key is an off-curve encoding must make
    // verification fail at the point-decode boundary, not verify or panic.
    let (_, sig) = signed_fixture();
    let mut poisoned = sig.clone();
    // Poison a member other than wherever the real signer landed by corrupting
    // every position in turn would be simplest, but one off-curve member is
    // enough: to_point() on it returns Err before any challenge math.
    poisoned.ring[0] = RingMember {
        public_key: ALL_MAX,
    };
    assert!(
        poisoned.verify(b"msg").is_err(),
        "a ring containing an off-curve member key must fail to verify"
    );
}

/// A genuine small-subgroup member key must be refused at the decode boundary.
///
/// Contrast with `ring_signature_rejects_identity_member_key`, which is
/// `Ok(false)`: a torsion encoding is not a valid Ristretto point at all, so it
/// is rejected before any challenge math, whereas the identity decodes fine and
/// is refused by the signature math. Asserting the specific outcome is what
/// distinguishes the two classes — `is_err()` alone would not have noticed that
/// the old fixture was the identity.
///
/// Breaks if: `RingMember::to_point` stops rejecting non-canonical encodings —
/// e.g. a refactor from Ristretto back to raw Edwards, which would reintroduce
/// the whole small-subgroup surface.
#[test]
fn ring_signature_rejects_order_8_torsion_member_key() {
    let (_, sig) = signed_fixture();
    let mut poisoned = sig.clone();
    poisoned.ring[0] = RingMember {
        public_key: EDWARDS_ORDER_8_TORSION,
    };
    match poisoned.verify(b"msg") {
        Err(PrivacyError::InvalidKey(_)) => {},
        other => panic!(
            "expected InvalidKey at the decode boundary for a torsion member key, got {other:?}"
        ),
    }
}

#[test]
fn ring_signature_rejects_empty_ring() {
    let sig = MLSAGSignature {
        ring: vec![],
        key_image: KeyImage::from_bytes(&valid_point_bytes(5)),
        c: vec![0u8; 32],
        s: vec![],
    };
    match sig.verify(b"msg") {
        Err(PrivacyError::RingSignature(_)) => {},
        other => panic!("expected RingSignature error for empty ring, got {other:?}"),
    }
}

#[test]
fn ring_signature_rejects_wrong_length_response_scalar() {
    // A crafted peer-supplied signature with a short response scalar must be
    // rejected, not panic with an index-out-of-bounds.
    let (_, sig) = signed_fixture();
    let mut poisoned = sig.clone();
    poisoned.s[0] = vec![0u8; 31];
    match poisoned.verify(b"msg") {
        Err(PrivacyError::RingSignature(_)) => {},
        other => panic!("expected RingSignature error for short scalar, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Key-image reuse — double-spend detection
// ---------------------------------------------------------------------------

#[test]
fn key_image_reuse_is_detected() {
    // The double-spend surface. The second registration of the same key image
    // must be rejected, or a spent output can be spent again.
    let registry = KeyImageRegistry::new();
    let image = KeyImage::from_bytes(&valid_point_bytes(42));

    registry.register(&image).expect("first spend registers");
    assert!(registry.is_spent(&image));

    match registry.register(&image) {
        Err(PrivacyError::DoubleSpend) => {},
        Ok(()) => panic!("re-registering a key image must be rejected — double spend"),
        other => panic!("expected DoubleSpend, got {other:?}"),
    }
    assert_eq!(
        registry.count(),
        1,
        "a rejected reuse must not grow the set"
    );
}

#[test]
fn distinct_key_images_coexist() {
    // Control for the reuse test: two different key images must both register,
    // or the reuse test could pass by rejecting everything.
    let registry = KeyImageRegistry::new();
    let a = KeyImage::from_bytes(&valid_point_bytes(1));
    let b = KeyImage::from_bytes(&valid_point_bytes(2));
    registry.register(&a).unwrap();
    registry.register(&b).unwrap();
    assert_eq!(registry.count(), 2);
}

// ---------------------------------------------------------------------------
// Identity driven through the PRODUCTION verification path
// ---------------------------------------------------------------------------
//
// Review of #2237, verbatim: "The identity tests construct and distinguish the
// identity … They never pass identity into the verification or public-input
// path whose contract requires identity to be rejected."
//
// That is correct, and it is the same defect as #2235's: a check performed on
// the object rather than through the path whose contract is at issue.
// Constructing the identity and observing it is distinguishable proves the
// constructor works. It says nothing about whether a verifier refuses it.
//
// Under Ristretto the all-zero encoding IS a valid point, so `to_point()` and
// the key-image decode both SUCCEED on it — identity is not filtered at the
// decode boundary the way an off-curve or torsion encoding is. Whether it is
// refused therefore depends on the verification path itself, which is precisely
// why it has to be driven through `MLSAGSignature::verify` rather than checked
// beside it.

/// Identity as a ring member's public key must not verify.
///
/// Breaks if: `MLSAGSignature::verify` stops binding ring members into the
/// challenge chain. Note this is `Ok(false)` and not `Err` — the identity
/// decodes successfully, so it is refused by the signature math, not by the
/// decode guard. Asserting the exact outcome matters: a future explicit
/// identity check at `to_point()` would change this to `Err`, and that should
/// be a deliberate edit to this test, not a silent pass.
#[test]
fn ring_signature_rejects_identity_member_key() {
    let (_, sig) = signed_fixture();
    let mut poisoned = sig.clone();
    poisoned.ring[0] = RingMember {
        public_key: ALL_ZERO,
    };
    match poisoned.verify(b"msg") {
        Ok(false) => {},
        Ok(true) => panic!("a ring containing the identity as a member key must not verify"),
        Err(e) => panic!(
            "expected a clean rejection through the verification path, got an error: {e:?}"
        ),
    }
}

/// Identity as the key image must not verify.
///
/// The key image is the double-spend guard: it is what makes two spends of one
/// output linkable. An identity key image that verified would be a key image
/// any signer could produce, collapsing linkability.
///
/// Breaks if: `MLSAGSignature::verify` stops binding the key image into the
/// challenge chain.
#[test]
fn ring_signature_rejects_identity_key_image() {
    let (_, sig) = signed_fixture();
    let mut poisoned = sig.clone();
    poisoned.key_image = KeyImage { image: ALL_ZERO };
    match poisoned.verify(b"msg") {
        Ok(false) => {},
        Ok(true) => panic!("an identity key image must not verify"),
        Err(e) => panic!("expected a clean rejection, got an error: {e:?}"),
    }
}

/// Identity reaches the decode boundary without being filtered — stated as a
/// test rather than left implicit, because it is the reason the two tests above
/// have to exist at all.
///
/// Breaks if: an explicit identity check is added at `to_point()`. That would be
/// a strengthening, and the two tests above should then expect `Err`.
#[test]
fn identity_is_not_filtered_at_the_ristretto_decode_boundary() {
    assert!(
        CompressedRistretto(ALL_ZERO).decompress().is_some(),
        "the all-zero encoding is a valid Ristretto point (the identity) — \
         so decode cannot be what rejects it, and verification must"
    );
    assert!(
        CompressedRistretto(EDWARDS_ORDER_8_TORSION)
            .decompress()
            .is_none(),
        "by contrast, a torsion encoding IS filtered at decode"
    );
}
