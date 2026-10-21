// End-to-end integration tests for the privacy subsystem (#973).
//
// Covers:
//   1. Ring signature full flow (ring sizes 7/8/16, wrong-message rejection,
//      wrong-ring rejection)
//   2. Stealth address flow (create → scan → detect → recover private key)
//   3. Key-image double-spend prevention via KeyImageRegistry
//   4. Confidential transfer roundtrip (Pedersen commit + range proof +
//      serialize/deserialize)
//
// Run:
//   cargo test --test privacy_e2e -p wrfcoin-privacy

use curve25519_dalek::{constants::RISTRETTO_BASEPOINT_TABLE, scalar::Scalar};
use rand_core::{OsRng, RngCore};
use wrfcoin_privacy::{
    ring_signatures::{KeyImage, MLSAGSignature, RingMember, MIN_RING_SIZE},
    stealth_addresses::{StealthAddress, StealthKeys},
    key_image_registry::KeyImageRegistry,
    pedersen::PedersenCommitment,
    range_proof::RangeProof,
    error::PrivacyError,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a random (private_key_bytes, public_key_bytes) keypair.
fn gen_keypair() -> ([u8; 32], [u8; 32]) {
    let mut buf = [0u8; 64];
    OsRng.fill_bytes(&mut buf);
    let scalar = Scalar::from_bytes_mod_order_wide(&buf);
    let public = (&scalar * RISTRETTO_BASEPOINT_TABLE).compress().to_bytes();
    (scalar.to_bytes(), public)
}

/// Build a Vec of `n` decoy RingMembers.
fn make_decoys(n: usize) -> Vec<RingMember> {
    (0..n)
        .map(|_| {
            let (_, pub_key) = gen_keypair();
            RingMember { public_key: pub_key }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 1. Ring signature full flow
// ---------------------------------------------------------------------------

/// Sign + verify with the minimum allowed ring size (7 total = 6 decoys + signer).
#[test]
fn ring_sig_min_ring_sign_and_verify() {
    let (priv_key, _) = gen_keypair();
    let decoys = make_decoys(MIN_RING_SIZE - 1);
    let msg = b"ring-e2e-min";

    let sig = MLSAGSignature::sign(msg, &priv_key, &decoys)
        .expect("sign with MIN_RING_SIZE must succeed");
    assert!(
        sig.verify(msg).expect("verify must not error"),
        "signature must pass verification"
    );
}

/// Sign + verify with ring size 8 (7 decoys + signer).
#[test]
fn ring_sig_size_8_sign_and_verify() {
    let (priv_key, _) = gen_keypair();
    let decoys = make_decoys(7);
    let msg = b"ring-e2e-8";

    let sig = MLSAGSignature::sign(msg, &priv_key, &decoys)
        .expect("sign with ring size 8 must succeed");
    assert!(sig.verify(msg).expect("verify must not error"));
}

/// Sign + verify with ring size 16 (15 decoys + signer).
#[test]
fn ring_sig_size_16_sign_and_verify() {
    let (priv_key, _) = gen_keypair();
    let decoys = make_decoys(15);
    let msg = b"ring-e2e-16";

    let sig = MLSAGSignature::sign(msg, &priv_key, &decoys)
        .expect("sign with ring size 16 must succeed");
    assert!(sig.verify(msg).expect("verify must not error"));
}

/// Ring size below MIN_RING_SIZE (4 total) must be rejected by sign().
#[test]
fn ring_sig_below_minimum_rejected() {
    let (priv_key, _) = gen_keypair();
    // 3 decoys + signer = 4 total, below MIN_RING_SIZE = 7.
    let decoys = make_decoys(3);
    let result = MLSAGSignature::sign(b"too-small", &priv_key, &decoys);
    assert!(
        result.is_err(),
        "ring size below minimum must return Err"
    );
}

/// Verifying a valid signature against the wrong message must return false.
#[test]
fn ring_sig_wrong_message_fails_verification() {
    let (priv_key, _) = gen_keypair();
    let decoys = make_decoys(MIN_RING_SIZE - 1);

    let sig = MLSAGSignature::sign(b"original-message", &priv_key, &decoys)
        .expect("sign must succeed");
    let valid = sig.verify(b"tampered-message").expect("verify must not error");
    assert!(!valid, "signature signed over different message must not verify");
}

/// Replacing ring members after signing must break verification.
#[test]
fn ring_sig_wrong_ring_fails_verification() {
    let (priv_key, _) = gen_keypair();
    let decoys = make_decoys(MIN_RING_SIZE - 1);
    let msg = b"original-msg";

    let mut sig = MLSAGSignature::sign(msg, &priv_key, &decoys)
        .expect("sign must succeed");

    // Replace every ring member's public key with a random one.
    for member in sig.ring.iter_mut() {
        let (_, new_pub) = gen_keypair();
        member.public_key = new_pub;
    }

    let valid = sig.verify(msg).unwrap_or(false);
    assert!(!valid, "signature with substituted ring must not verify");
}

/// The key image embedded in the signature must match the standalone KeyImage.
#[test]
fn ring_sig_key_image_consistency() {
    let (priv_key, _) = gen_keypair();
    let decoys = make_decoys(MIN_RING_SIZE - 1);

    let standalone = KeyImage::generate(&priv_key)
        .expect("key image generation must succeed");
    let sig = MLSAGSignature::sign(b"ki-check", &priv_key, &decoys)
        .expect("sign must succeed");

    assert_eq!(
        sig.key_image, standalone,
        "key image in signature must match standalone KeyImage::generate"
    );
}

// ---------------------------------------------------------------------------
// 2. Stealth address flow
// ---------------------------------------------------------------------------

/// Full sender→recipient flow:
///   generate keys → create stealth address → recipient scans and detects
///   → recipient recovers one-time private key → verify key corresponds to
///   the one-time public key on-chain.
#[test]
fn stealth_address_full_flow() {
    // Recipient publishes stealth keys.
    let recipient = StealthKeys::generate();

    // Sender creates a one-time stealth address for this recipient.
    let (addr, _ephemeral_secret) = StealthAddress::new(
        recipient.public_spend_key,
        recipient.public_view_key,
        None,
    )
    .expect("StealthAddress::new must succeed");

    // Recipient scans: detects the payment is addressed to them.
    let detected = addr
        .is_for_recipient(&recipient.private_view_key, &recipient.public_spend_key)
        .expect("scan must not error");
    assert!(detected, "recipient must detect payment addressed to them");

    // Recipient recovers the one-time private key.
    let one_time_priv = addr
        .recover_private_key(&recipient.private_view_key, &recipient.private_spend_key)
        .expect("key recovery must succeed");

    // Verify: one_time_priv * G == addr.one_time_public_key.
    let scalar = Scalar::from_bytes_mod_order(one_time_priv);
    let derived_pub = (&scalar * RISTRETTO_BASEPOINT_TABLE).compress().to_bytes();
    assert_eq!(
        derived_pub, addr.one_time_public_key,
        "recovered private key must correspond to the on-chain one-time public key"
    );
}

/// A bystander must not detect a payment addressed to a different recipient.
#[test]
fn stealth_address_bystander_cannot_detect() {
    let recipient = StealthKeys::generate();
    let bystander = StealthKeys::generate();

    let (addr, _) = StealthAddress::new(
        recipient.public_spend_key,
        recipient.public_view_key,
        None,
    )
    .expect("StealthAddress::new must succeed");

    let detected = addr
        .is_for_recipient(&bystander.private_view_key, &bystander.public_spend_key)
        .expect("scan must not error");
    assert!(!detected, "bystander must not detect payment addressed to another");
}

/// Each call to StealthAddress::new produces a unique one-time key (fresh
/// ephemeral scalar every time).
#[test]
fn stealth_address_uniqueness_per_transfer() {
    let recipient = StealthKeys::generate();

    let (addr1, _) =
        StealthAddress::new(recipient.public_spend_key, recipient.public_view_key, None)
            .expect("first transfer must succeed");
    let (addr2, _) =
        StealthAddress::new(recipient.public_spend_key, recipient.public_view_key, None)
            .expect("second transfer must succeed");

    assert_ne!(
        addr1.one_time_public_key, addr2.one_time_public_key,
        "each transfer must produce a unique one-time address"
    );
    assert_ne!(
        addr1.transaction_key, addr2.transaction_key,
        "each transfer must use a unique ephemeral transaction key"
    );
}

/// Stealth address serializes and deserializes correctly; recipient can still
/// detect and spend after roundtrip.
#[test]
fn stealth_address_json_roundtrip_and_detect() {
    let recipient = StealthKeys::generate();
    let (addr, _) =
        StealthAddress::new(recipient.public_spend_key, recipient.public_view_key, None)
            .expect("StealthAddress::new must succeed");

    let json = addr.to_json().expect("serialization must succeed");
    let addr2 = StealthAddress::from_json(&json).expect("deserialization must succeed");

    assert_eq!(addr, addr2, "deserialized address must equal original");
    assert!(
        addr2
            .is_for_recipient(&recipient.private_view_key, &recipient.public_spend_key)
            .expect("scan must not error"),
        "recipient must detect payment after JSON roundtrip"
    );
}

// ---------------------------------------------------------------------------
// 3. Key-image double-spend prevention
// ---------------------------------------------------------------------------

/// Signing the same message twice with the same private key produces identical
/// key images; the second registration must be rejected.
#[test]
fn key_image_double_spend_prevention() {
    let (priv_key, _) = gen_keypair();
    let decoys = make_decoys(MIN_RING_SIZE - 1);
    let msg = b"spend-once";

    let sig1 = MLSAGSignature::sign(msg, &priv_key, &decoys)
        .expect("first sign must succeed");
    let sig2 = MLSAGSignature::sign(msg, &priv_key, &decoys)
        .expect("second sign must succeed");

    // Both signatures carry the same key image.
    assert_eq!(
        sig1.key_image, sig2.key_image,
        "same private key must produce identical key images"
    );

    // Registry rejects the second registration.
    let registry = KeyImageRegistry::new();
    registry
        .register(&sig1.key_image)
        .expect("first registration must succeed");

    let result = registry.register(&sig2.key_image);
    assert!(
        result.is_err(),
        "second registration with identical key image must be rejected"
    );
    match result.unwrap_err() {
        PrivacyError::DoubleSpend => {} // expected
        other => panic!("expected DoubleSpend error, got: {:?}", other),
    }
}

/// Distinct private keys produce distinct key images; all register without error.
#[test]
fn key_image_distinct_keys_all_accepted() {
    let registry = KeyImageRegistry::new();

    for _ in 0..10 {
        let (priv_key, _) = gen_keypair();
        let ki = KeyImage::generate(&priv_key).expect("key image generation must succeed");
        registry.register(&ki).expect("distinct key image must register without error");
    }
    assert_eq!(registry.count(), 10);
}

/// is_spent returns false before registration and true after.
#[test]
fn key_image_registry_is_spent_lifecycle() {
    let (priv_key, _) = gen_keypair();
    let ki = KeyImage::generate(&priv_key).expect("key image generation");

    let registry = KeyImageRegistry::new();
    assert!(!registry.is_spent(&ki), "must not be spent before registration");

    registry.register(&ki).expect("registration must succeed");
    assert!(registry.is_spent(&ki), "must be spent after registration");
}

// ---------------------------------------------------------------------------
// 4. Confidential transfer roundtrip (Pedersen + range proof)
// ---------------------------------------------------------------------------

/// Create a Pedersen commitment, generate a range proof, verify it, then
/// serialize/deserialize the proof and verify again.
#[test]
fn confidential_transfer_roundtrip() {
    let value: u64 = 12_345;
    let n_bits: usize = 16; // covers values 0..65535

    // Create a Pedersen commitment with a random blinding factor.
    let (commitment, blinding) = PedersenCommitment::commit(value);
    assert!(commitment.verify(value, &blinding), "fresh commitment must verify");

    // Prove that the committed value is in [0, 2^16).
    let proof = RangeProof::prove(value, &blinding, n_bits)
        .expect("range proof for valid value must succeed");

    // Verify the range proof against the commitment.
    let proof_valid = proof
        .verify(&commitment)
        .expect("range proof verification must not error");
    assert!(proof_valid, "range proof must verify for committed value");

    // Serialize → deserialize → verify again.
    let json = serde_json::to_string(&proof).expect("proof serialization must succeed");
    let proof2: RangeProof =
        serde_json::from_str(&json).expect("proof deserialization must succeed");

    let proof2_valid = proof2
        .verify(&commitment)
        .expect("deserialized proof verification must not error");
    assert!(proof2_valid, "deserialized range proof must still verify");
}

/// A range proof for one value must not verify against a commitment to a
/// different value.
#[test]
fn confidential_transfer_wrong_value_rejected() {
    let value: u64 = 100;
    let other: u64 = 101;
    let n_bits = 16;

    let (commitment_100, blinding) = PedersenCommitment::commit(value);
    let (commitment_101, _) = PedersenCommitment::commit(other);

    let proof = RangeProof::prove(value, &blinding, n_bits)
        .expect("prove must succeed for valid value");

    // Proof was made for 100; verifying against commitment to 101 must fail.
    let valid = proof
        .verify(&commitment_101)
        .expect("verify must not error");
    assert!(
        !valid,
        "range proof for value=100 must not verify against commitment to value=101"
    );
    // But it must still verify against the original commitment.
    assert!(proof.verify(&commitment_100).unwrap());
}

/// Pedersen commitment serializes and deserializes correctly (point roundtrip).
#[test]
fn pedersen_commitment_bytes_roundtrip() {
    let (commitment, _blinding) = PedersenCommitment::commit(999);

    let bytes = commitment.to_bytes();
    assert_eq!(bytes.len(), 32, "commitment bytes must be exactly 32 bytes");

    let recovered =
        PedersenCommitment::from_bytes(&bytes).expect("from_bytes must succeed for valid point");

    // The recovered commitment (same point, zero blinding) verifies against
    // the original blinding and value.
    assert!(
        recovered.commitment == commitment.commitment,
        "deserialized commitment point must equal original"
    );
}

/// Homomorphic addition: Commit(a) + Commit(b) == Commit(a + b) with combined
/// blinding factors.
#[test]
fn pedersen_homomorphic_addition() {
    let (c_a, r_a) = PedersenCommitment::commit(300);
    let (c_b, r_b) = PedersenCommitment::commit(400);
    let r_sum = r_a + r_b;

    let c_sum = c_a + c_b;
    assert!(
        c_sum.verify(700, &r_sum),
        "C(300) + C(400) must equal C(700) with combined blinding"
    );
}

// #3413 S0: freeze legacy behavior before the separately gated indexed schema.
// The known same-R collision below is documented, not fixed by this slice.
fn legacy_stealth_pin_3413() -> StealthAddress {
    serde_json::from_str(include_str!("fixtures/stealth_legacy_3413.json")).unwrap()
}

#[test]
fn legacy_stealth_functions_unchanged_golden_3413() {
    let address = legacy_stealth_pin_3413();
    let (view, spend) = (Scalar::from(3u64).to_bytes(), Scalar::from(5u64).to_bytes());
    assert!(address
        .is_for_recipient(&view, &address.public_spend_key)
        .unwrap());
    let recovered = address.recover_private_key(&view, &spend).unwrap();
    let recovered_public = (&Scalar::from_bytes_mod_order(recovered) * RISTRETTO_BASEPOINT_TABLE)
        .compress()
        .to_bytes();
    assert_eq!(recovered_public, address.one_time_public_key);
    assert_eq!(
        address.decrypt_payment_id(&view).unwrap(),
        Some(*b"S0-ID001")
    );
    let unrelated_view = Scalar::from(11u64).to_bytes();
    assert!(!address
        .is_for_recipient(&unrelated_view, &address.public_spend_key)
        .unwrap());
    assert_eq!(
        StealthAddress::from_json(&address.to_json().unwrap()).unwrap(),
        address
    );
}

#[test]
fn legacy_stealth_two_outputs_same_r_collide_3413() {
    let first = legacy_stealth_pin_3413();
    let mut second = legacy_stealth_pin_3413();
    // Distinct sender metadata cannot distinguish the legacy spend-key offset.
    second.payment_id.as_mut().unwrap()[0] ^= 1;
    let (view, spend) = (Scalar::from(3u64).to_bytes(), Scalar::from(5u64).to_bytes());
    assert_ne!(first.payment_id, second.payment_id);
    assert_eq!(first.transaction_key, second.transaction_key);
    assert!(second
        .is_for_recipient(&view, &second.public_spend_key)
        .unwrap());
    let first_spend = first.recover_private_key(&view, &spend).unwrap();
    let second_spend = second.recover_private_key(&view, &spend).unwrap();
    // Compare silently: never put even synthetic private scalars in diagnostics.
    assert!(first_spend == second_spend);
    let first_image = KeyImage::generate(&first_spend).unwrap();
    let second_image = KeyImage::generate(&second_spend).unwrap();
    assert_eq!(first_image, second_image);
}

#[test]
fn legacy_stealth_new_preserves_legacy_formula_3413() {
    use curve25519_dalek::ristretto::CompressedRistretto;
    use sha3::{Digest, Sha3_256};
    let pin = legacy_stealth_pin_3413();
    let payment = *b"S0-ID001";
    let (address, ephemeral_fixture) =
        StealthAddress::new(pin.public_spend_key, pin.public_view_key, Some(payment)).unwrap();
    let r = Scalar::from_bytes_mod_order(ephemeral_fixture);
    let a = CompressedRistretto(pin.public_view_key)
        .decompress()
        .unwrap();
    let b = CompressedRistretto(pin.public_spend_key)
        .decompress()
        .unwrap();
    // Independent legacy framing: SHA3-256(compress(rA)), no tag or index.
    let shared: [u8; 32] = Sha3_256::digest((r * a).compress().as_bytes()).into();
    let expected_public = ((&Scalar::from_bytes_mod_order(shared) * RISTRETTO_BASEPOINT_TABLE) + b)
        .compress()
        .to_bytes();
    assert_eq!(address.one_time_public_key, expected_public);
    assert_eq!(
        address.transaction_key,
        (&r * RISTRETTO_BASEPOINT_TABLE).compress().to_bytes()
    );
    let mut encrypted = payment;
    for i in 0..8 {
        encrypted[i] ^= shared[i];
    }
    assert_eq!(address.payment_id, Some(encrypted));
    let view_fixture = Scalar::from(3u64).to_bytes();
    assert_eq!(
        address.decrypt_payment_id(&view_fixture).unwrap(),
        Some(payment)
    );
}
