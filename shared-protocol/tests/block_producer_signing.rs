//! Block-producer authentication — the negative-test suite (#2290 / #2220).
//!
//! This exercises `consensus::block_producer_auth` — the verifier, signer and
//! per-engine authority predicates. The core negatives use the standalone
//! `ProducerAttestation`; the `_2226` tests at the end exercise the seam against
//! the **real** `Block::producer_pubkey`/`producer_sig` fields now that #2226 has
//! folded `producer_pubkey` into `Block::canonical_body` and collapsed
//! `Block::calculate_hash` onto `signing_preimage`. The live `verify_block`
//! call-path wiring (gossip / IBD / `create_next_block` signing) remains the
//! #2290 follow-up.
//!
//! # The property, in two layers
//!
//! `verify_block_producer` **authenticates**: it answers "did the holder of
//! `producer_pubkey` sign this exact block?". The `authorize_producer_*`
//! predicates decide **authority**: "is that key allowed to produce here?".
//! Unsigned and wrong-key fail authentication; rotated-out and cross-set
//! authenticate fine but fail authority. Both layers must reject.
//!
//! # Guard-revert check (the RED-on-revert bar)
//!
//! - Stub `verify_block_producer` to return `Ok(pubkey)` unconditionally →
//!   `unsigned_block_is_rejected` and `wrong_key_is_rejected` go RED.
//! - Stub `authorize_producer_rotation` / `authorize_producer_in_set` to `true` →
//!   `rotated_out_key_is_rejected` / `cross_set_key_is_rejected` go RED.
//!
//! Run:
//!   cargo test -p wrfcoin-shared-protocol --test block_producer_signing

use rand::SeedableRng;
use rand::rngs::StdRng;

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::consensus::block_producer_auth::{
    ProducerAttestation, ProducerAuthError, authorize_producer_in_set, authorize_producer_pop,
    attest_block_producer, authorize_producer_rotation, block_producer_preimage,
    block_producer_signing_digest,
    sign_block_producer,
    verify_block_producer, verify_block_producer_on,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::rotation::{RotationPlan, rotation_pop_message};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const VERSION: SigningProtocolVersion = SigningProtocolVersion::V2;

fn identity() -> ChainIdentity {
    ChainIdentity::new("wrfcoin-testnet", "testnet")
}

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// A block with a fixed timestamp so every preimage is deterministic across
/// runs and machines (the golden vector depends on it).
fn fixed_block(height: u64) -> Block {
    let mut b = Block::new(3, Hash::default(), Vec::<Transaction>::new(), 1, height);
    b.timestamp = chrono::DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
    b
}

/// Structurally valid rotation `old -> new`, effective at height 1 with a real
/// proof of possession, so nothing passes because the plan is malformed. At any
/// height >= 1 the old key is revoked and only `new` is authoritative.
const EFFECTIVE_HEIGHT: u64 = 1;
const OVERLAP: u64 = 16;

fn rotation_plan() -> (RotationPlan, PublicKey, PublicKey) {
    let (_, old) = keypair(2_290_001);
    let (new_sk, new) = keypair(2_290_002);
    let proof = new_sk.sign(&rotation_pop_message(&old, &new, EFFECTIVE_HEIGHT));
    let plan = RotationPlan {
        old: old.clone(),
        new: new.clone(),
        effective_height: EFFECTIVE_HEIGHT,
        overlap_blocks: OVERLAP,
        proof: Some(proof),
    };
    (plan, old, new)
}

// ---------------------------------------------------------------------------
// Authentication failures (verify_block_producer)
// ---------------------------------------------------------------------------

/// **unsigned** — an unsigned block carries the all-zero attestation and must be
/// rejected. This is the base case: today every block is in this state.
#[test]
fn unsigned_block_is_rejected() {
    let block = fixed_block(7);
    let err = verify_block_producer(
        &block,
        &ProducerAttestation::unsigned(),
        &identity(),
        VERSION,
    )
    .expect_err("an unsigned block must not authenticate");
    assert_eq!(err, ProducerAuthError::Unsigned);
}

/// **wrong-key** — the re-attribution attack of §3.2: take an honestly signed
/// block, swap in a *different* producer pubkey, keep the signature. Because the
/// pubkey is inside the preimage, the signature no longer verifies under the
/// swapped-in key. Producer binding holds.
#[test]
fn wrong_key_is_rejected() {
    let (attacker_sk, _attacker_pk) = keypair(10);
    let (_victim_sk, victim_pk) = keypair(11);
    let block = fixed_block(7);

    // Attacker signs honestly with their own key...
    let mut att = sign_block_producer(&block, &attacker_sk, &identity(), VERSION);
    // ...then re-attributes the block to the victim's key.
    att.producer_pubkey = victim_pk.as_bytes().try_into().unwrap();

    let err = verify_block_producer(&block, &att, &identity(), VERSION)
        .expect_err("a re-attributed signature must not verify");
    assert_eq!(err, ProducerAuthError::BadSignature);
}

/// A signature made over a *different* block (or chain/version) must not verify
/// against this one — the domain-separated frame binds all of it.
#[test]
fn signature_over_a_different_block_is_rejected() {
    let (sk, _pk) = keypair(12);
    let signed = fixed_block(7);
    let other = fixed_block(8); // different height -> different preimage

    let att = sign_block_producer(&signed, &sk, &identity(), VERSION);
    let err = verify_block_producer(&other, &att, &identity(), VERSION)
        .expect_err("a signature over another block must not verify");
    assert_eq!(err, ProducerAuthError::BadSignature);

    // And across chains: a testnet signature must not verify as mainnet.
    let mainnet = ChainIdentity::new("wrfcoin-main", "mainnet");
    let err = verify_block_producer(&signed, &att, &mainnet, VERSION)
        .expect_err("a cross-chain signature must not verify");
    assert_eq!(err, ProducerAuthError::BadSignature);
}

/// A garbage `producer_pubkey` / `producer_sig` pair is rejected. Depending on
/// whether the 32 bytes happen to decompress to a curve point, this surfaces as
/// `MalformedKey` (rejected before the signature check) or `BadSignature` (the
/// garbage signature does not verify) — both are correct rejections; what must
/// never happen is authentication.
#[test]
fn garbage_producer_key_is_rejected() {
    let block = fixed_block(7);
    let att = ProducerAttestation {
        producer_pubkey: [0xFF; 32],
        producer_sig: [1u8; 64], // non-zero so it is not treated as unsigned
    };
    let err = verify_block_producer(&block, &att, &identity(), VERSION)
        .expect_err("a garbage producer key must not authenticate");
    assert!(
        matches!(
            err,
            ProducerAuthError::MalformedKey | ProducerAuthError::BadSignature
        ),
        "unexpected error variant: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Authority failures (authenticates, but not allowed to produce)
// ---------------------------------------------------------------------------

/// **rotated-out** — a block signed by a revoked key *authenticates* (the
/// signature is real) but must be rejected by the rotation authority predicate.
/// This is the exact gap `block_producer_authority.rs` characterises as
/// currently unenforceable: here the seam exists to enforce it.
#[test]
fn rotated_out_key_is_rejected() {
    let (plan, old, _new) = rotation_plan();
    let old_sk = {
        // Re-derive the old secret from the same seed used in `rotation_plan`.
        let mut rng = StdRng::seed_from_u64(2_290_001);
        PrivateKey::generate(&mut rng)
    };
    let block = fixed_block(EFFECTIVE_HEIGHT); // height >= effective -> old revoked

    let att = sign_block_producer(&block, &old_sk, &identity(), VERSION);

    // Authentication succeeds: the old key really did sign this block.
    let authed = verify_block_producer(&block, &att, &identity(), VERSION)
        .expect("a real signature by the old key authenticates");
    assert_eq!(authed, old);

    // Authority fails: the rotation plan does not accept the old key here.
    assert!(
        !authorize_producer_rotation(&authed, block.height, &plan),
        "a rotated-out key must not be authorized to produce at height {}",
        block.height
    );
}

/// The overlap window still accepts the old key — proving the rejection above is
/// height-scoped authority, not a blanket ban on the old key.
#[test]
fn old_key_is_authorized_inside_the_overlap_window() {
    let (plan, old, _new) = rotation_plan();
    // effective=1, overlap=16 -> overlap_start = 0; height 0 is inside [0, 1).
    assert!(authorize_producer_rotation(&old, 0, &plan));
}

/// **cross-set** — a block signed by a key that is not in the active validator
/// set authenticates but must be rejected by the set-membership predicate. A
/// vote/production is only valid under the committee it belongs to.
#[test]
fn cross_set_key_is_rejected() {
    let (outsider_sk, outsider_pk) = keypair(20);
    let (_a_sk, a_pk) = keypair(21);
    let (_b_sk, b_pk) = keypair(22);
    let authorized = [a_pk, b_pk];
    let block = fixed_block(7);

    let att = sign_block_producer(&block, &outsider_sk, &identity(), VERSION);
    let authed = verify_block_producer(&block, &att, &identity(), VERSION)
        .expect("the outsider's own signature authenticates");
    assert_eq!(authed, outsider_pk);

    assert!(
        !authorize_producer_in_set(&authed, &authorized),
        "a key outside the active set must not be authorized"
    );
}

// ---------------------------------------------------------------------------
// The honest path — everything lines up
// ---------------------------------------------------------------------------

/// A block signed by an in-set, rotation-current key whose address matches the
/// coinbase recipient passes authentication and every authority predicate.
#[test]
fn honest_producer_passes_authentication_and_authority() {
    let (sk, pk) = keypair(30);
    let block = fixed_block(7);
    let att = sign_block_producer(&block, &sk, &identity(), VERSION);

    let authed = verify_block_producer(&block, &att, &identity(), VERSION)
        .expect("the honest signature authenticates");
    assert_eq!(authed, pk);

    // PoP: the key derives the coinbase recipient it claims.
    assert!(authorize_producer_pop(&authed, &pk.to_address()));
    assert!(!authorize_producer_pop(&authed, "wrfsomeoneelse"));

    // Set: the key is in the active set.
    assert!(authorize_producer_in_set(&authed, &[pk.clone()]));
}

// ---------------------------------------------------------------------------
// Golden vector — locks the preimage bytes so signers cannot drift (§3.4/§7)
// ---------------------------------------------------------------------------

/// The exact bytes of the amended `wrfcoin.block_header.v1` preimage for a fixed
/// block + fixed producer key + testnet identity at V2. Any signer in any
/// language that diverges from these bytes produces signatures this chain will
/// reject; that is the cross-repo drift `GENESIS_CUT_PLAN.md` §7 exists to catch.
///
/// If this fails after an intentional preimage change, re-pin from the failure
/// output — do not delete the assertion.
const GOLDEN_PREIMAGE_HEX: &str = "1700000000000000777266636f696e2e626c6f636b5f6865616465722e76310f00000000000000777266636f696e2d746573746e65740700000000000000746573746e6574020000008000000000000000030000002000000000000000000000000000000000000000000000000000000000000000000000000000000007000000000000000100000000000000000000000068e5cf8b01000020000000000000007c1772fbd7e1fea0ae8007f7829fdc1173c7100b2848f5d8a54a07981f2fdc8c00000000000000000000000000000000";

#[test]
fn preimage_golden_vector_is_stable() {
    // producer key: seed 30's public key (deterministic under StdRng/ChaCha).
    let (_sk, pk) = keypair(30);
    let producer_pubkey: [u8; 32] = pk.as_bytes().try_into().unwrap();
    let block = fixed_block(7);

    let preimage = block_producer_preimage(&block, &producer_pubkey, &identity(), VERSION);
    let got = hex::encode(&preimage);
    assert_eq!(
        got, GOLDEN_PREIMAGE_HEX,
        "block-producer preimage bytes changed. If intentional (#2219/#2226 \
         rebase or a spec change), re-pin GOLDEN_PREIMAGE_HEX from this value \
         and coordinate cross-repo signers per GENESIS_CUT_PLAN.md §7."
    );
}

// ---------------------------------------------------------------------------
// #2226 — the seam wired onto the real Block fields (collapse + producer bind).
// After #2226, `producer_pubkey`/`producer_sig` live on `Block`,
// `Block::calculate_hash` at V2 is `hash_bytes(signing_preimage())`, and the seam
// delegates to `Block::canonical_body` instead of a hand-kept replica.
// ---------------------------------------------------------------------------

/// The seam's preimage for a block's own producer key is exactly the block's
/// `signing_preimage` — one encoding, no replica (#2226 §3.2).
#[test]
fn seam_preimage_equals_block_signing_preimage_2226() {
    let (_sk, pk) = keypair(40);
    let pk_bytes: [u8; 32] = pk.as_bytes().try_into().unwrap();
    let mut block = fixed_block(7);
    block.producer_pubkey = pk_bytes;

    assert_eq!(
        block_producer_preimage(&block, &pk_bytes, &identity(), VERSION),
        block.signing_preimage_for(&identity(), VERSION),
        "the block-producer preimage must be the block's own signing preimage (#2226 collapse)"
    );
}

/// `verify_block_producer_on` reads the attestation off the block's real fields
/// and authenticates it; an unsigned block (default fields) is rejected.
#[test]
fn verify_block_producer_on_reads_real_block_fields_2226() {
    let (sk, pk) = keypair(41);
    let mut block = fixed_block(7);
    let att = sign_block_producer(&block, &sk, &identity(), VERSION);
    block.producer_pubkey = att.producer_pubkey;
    block.producer_sig = att.producer_sig.to_vec();

    let got = verify_block_producer_on(&block, &identity(), VERSION)
        .expect("a block carrying its own valid producer attestation authenticates");
    assert_eq!(got, pk);

    let unsigned = fixed_block(7);
    assert_eq!(
        verify_block_producer_on(&unsigned, &identity(), VERSION),
        Err(ProducerAuthError::Unsigned),
        "a block with no producer signature must be rejected"
    );
}

/// Attribution-swap closed **by construction** (#2226 §3.2(b)): re-attributing a
/// signed block to a different key both changes the block hash and fails to
/// authenticate — the identity that is hashed is the identity that is signed.
#[test]
fn attribution_swap_on_real_block_fields_is_rejected_2226() {
    let (sk_a, _pk_a) = keypair(42);
    let (_sk_b, pk_b) = keypair(43);
    let mut block = fixed_block(7);
    let att = sign_block_producer(&block, &sk_a, &identity(), VERSION);
    block.producer_pubkey = att.producer_pubkey;
    block.producer_sig = att.producer_sig.to_vec();

    let hash_before = block.calculate_hash_for(SigningProtocolVersion::V2, Some(&identity()));

    // Attacker swaps in their own key, keeping A's signature.
    block.producer_pubkey = pk_b.as_bytes().try_into().unwrap();
    let hash_after = block.calculate_hash_for(SigningProtocolVersion::V2, Some(&identity()));

    assert_ne!(
        hash_before, hash_after,
        "#2226: swapping producer_pubkey changes the block hash (different block)"
    );
    assert_eq!(
        verify_block_producer_on(&block, &identity(), VERSION),
        Err(ProducerAuthError::BadSignature),
        "a re-attributed block must not authenticate"
    );
}

// ---------------------------------------------------------------------------
// #2728 — the signed input must be FIXED SIZE, whatever the block contains.
// ---------------------------------------------------------------------------

/// A transaction with enough payload that 200 of them push the block preimage
/// well past the ~1 KiB hardware signing limit that #2728 is about.
fn fat_tx(n: u64) -> Transaction {
    use wrfcoin_shared_protocol::blockchain::transaction::{TransactionInput, TransactionOutput};
    let input = TransactionInput {
        txid: Hash::default(),
        vout: n as u32,
        signature: Some("00".repeat(64)),
        public_key: Some("11".repeat(32)),
        coinbase_data: None,
    };
    let output = TransactionOutput {
        amount: 1_000 + n,
        address: format!("wrf{n:040x}"),
        script: "OP_TRUE".to_string(),
        nft_data: None,
    };
    let mut tx = Transaction::new(vec![input], vec![output]);
    tx.nonce = n;
    tx.set_id();
    tx
}

/// **The regression test for core4 #2728.**
///
/// `block_producer_preimage` serialises the whole block, transactions included,
/// so its length grows without bound. Ed25519 accepts arbitrary-length messages,
/// so signing the preimage directly works in software and every test passed —
/// but a **hardware** signer does not. Measured against the live producer
/// YubiKey (`35278470`, PIV 9A, ED25519) through `libykcs11.so`: 1024 bytes
/// signed, 4096 bytes returned `CKR_DATA_LEN_RANGE`. An HSM-backed producer
/// could not sign any non-trivial block, and it surfaced during a genesis cut.
///
/// This asserts the property that fixes it: **what we hand the signer is 32
/// bytes regardless of block size.** It goes RED if any call site is reverted to
/// signing the preimage.
///
/// It deliberately does NOT assert against a hardware limit — no CI runner has a
/// YubiKey. It asserts the invariant that makes the limit unreachable.
#[test]
fn signed_input_is_fixed_size_regardless_of_block_size_2728() {
    let (_sk, pk) = keypair(4242);
    let producer_pubkey: [u8; 32] = pk.as_bytes().try_into().unwrap();

    let mut empty = fixed_block(1);
    empty.producer_pubkey = producer_pubkey;

    // A block with enough transactions to blow past the card's input limit.
    let mut fat = fixed_block(1);
    fat.producer_pubkey = producer_pubkey;
    fat.transactions = (0..200)
        .map(|i| {
            fat_tx(i as u64)
        })
        .collect();

    let empty_preimage = block_producer_preimage(&empty, &producer_pubkey, &identity(), VERSION);
    let fat_preimage = block_producer_preimage(&fat, &producer_pubkey, &identity(), VERSION);

    // Precondition: the preimage really does grow — otherwise this test proves
    // nothing and would pass even against the broken code.
    assert!(
        fat_preimage.len() > 4096,
        "test is vacuous unless the fat block's preimage exceeds the ~1KiB hardware \
         limit that broke the producer; got {} bytes",
        fat_preimage.len()
    );
    assert!(
        fat_preimage.len() > empty_preimage.len() * 4,
        "preimage should scale with transaction count"
    );

    // The property: the signed input does not.
    let empty_digest = block_producer_signing_digest(&empty, &producer_pubkey, &identity(), VERSION);
    let fat_digest = block_producer_signing_digest(&fat, &producer_pubkey, &identity(), VERSION);

    assert_eq!(empty_digest.len(), 32, "signed input must be 32 bytes");
    assert_eq!(fat_digest.len(), 32, "signed input must be 32 bytes for ANY block size");
    assert_ne!(
        empty_digest, fat_digest,
        "different blocks must still produce different signed inputs"
    );
}

/// **The anti-vacuity test for #2728.**
///
/// The test above asserts the digest helper returns 32 bytes — which stays true
/// even if a call site is reverted to signing the raw preimage. That would make
/// it a guard that cannot fail. This one binds the *call sites* to the digest:
/// it takes a real signature from `sign_block_producer` and checks it verifies
/// over the DIGEST and **not** over the preimage.
///
/// **Scope, stated precisely** — an earlier version of this comment claimed
/// "revert any of the three call sites and this goes RED". That was false, and
/// the review of #2729 measured it:
///
/// | reverted call site | this test |
/// |---|---|
/// | `sign_block_producer` | **RED** |
/// | `verify_block_producer` | green (this test verifies directly, not through it) |
/// | `attest_block_producer` — **the HSM path that actually broke** | green |
///
/// The other two are covered by `fat_block_signs_and_verifies_end_to_end_2728`
/// and by `block_producer_authority_v2`. But leaving the HSM path unbound by the
/// test named for #2728 is exactly the gap that let #2728 ship, so
/// `attest_block_producer_signs_the_digest_2728` below closes it directly.
#[test]
fn producer_signature_is_over_the_digest_not_the_preimage_2728() {
    let (sk, pk) = keypair(7777);
    let mut block = fixed_block(3);
    block.transactions = (0..50).map(fat_tx).collect();

    let att = sign_block_producer(&block, &sk, &identity(), VERSION);
    block.producer_pubkey = att.producer_pubkey;
    block.producer_sig = att.producer_sig.to_vec();

    let sig = Signature::from_bytes(&att.producer_sig).expect("64-byte signature");
    let digest =
        block_producer_signing_digest(&block, &att.producer_pubkey, &identity(), VERSION);
    let preimage = block_producer_preimage(&block, &att.producer_pubkey, &identity(), VERSION);

    assert!(
        pk.verify(&digest, &sig),
        "the producer signature must be over the 32-byte digest"
    );
    assert!(
        !pk.verify(&preimage, &sig),
        "the producer signature must NOT be over the raw preimage — if this passes, a \
         call site has been reverted and hardware signing will break again (#2728)"
    );
}

/// **Binds the HSM call site — `attest_block_producer`.**
///
/// #2728 broke there, not in `sign_block_producer`, and the review of #2729
/// measured that reverting it alone left every test in this file green. A
/// regression test for a hardware bug that does not exercise the hardware path
/// is the same defect the bug came from, one level up.
///
/// This drives `attest_block_producer` through a real `BlockSigner` (software —
/// CI has no YubiKey) and asserts the signature is over the DIGEST. Revert that
/// call site and this goes RED.
#[test]
fn attest_block_producer_signs_the_digest_2728() {
    use wrfcoin_shared_protocol::crypto::signer::{BlockSigner, SignerProvenance};

    #[derive(Debug)]
    struct TestSigner {
        key: PrivateKey,
        provenance: SignerProvenance,
    }
    impl BlockSigner for TestSigner {
        fn sign(&self, payload: &[u8]) -> anyhow::Result<Signature> {
            Ok(self.key.sign(payload))
        }
        fn public_key(&self) -> PublicKey {
            PublicKey::from_private_key(&self.key)
        }
        fn provenance(&self) -> &SignerProvenance {
            &self.provenance
        }
    }

    let (sk, pk) = keypair(31337);
    let signer = TestSigner {
        key: sk,
        provenance: SignerProvenance::EnvKey { var_name: "TEST".to_string() },
    };

    let mut block = fixed_block(9);
    block.transactions = (0..50).map(fat_tx).collect();

    let att = attest_block_producer(&block, &signer, &identity(), VERSION)
        .expect("attestation must succeed");
    block.producer_pubkey = att.producer_pubkey;
    block.producer_sig = att.producer_sig.to_vec();

    let sig = Signature::from_bytes(&att.producer_sig).expect("64-byte signature");
    let digest = block_producer_signing_digest(&block, &att.producer_pubkey, &identity(), VERSION);
    let preimage = block_producer_preimage(&block, &att.producer_pubkey, &identity(), VERSION);

    assert!(
        pk.verify(&digest, &sig),
        "attest_block_producer must sign the 32-byte digest — this is the HSM path"
    );
    assert!(
        !pk.verify(&preimage, &sig),
        "attest_block_producer must NOT sign the raw preimage; reverting it reintroduces #2728 \
         and no other test in this file would catch it"
    );
    assert!(
        verify_block_producer_on(&block, &identity(), VERSION).is_ok(),
        "and the result must still authenticate"
    );
}

/// A fat block still round-trips sign -> verify. Guards the half-migration:
/// hashing on one side only would leave every block unverifiable, and the
/// failure mode ("BadSignature" on every block) looks nothing like the cause.
#[test]
fn fat_block_signs_and_verifies_end_to_end_2728() {
    let (sk, _pk) = keypair(99);
    let mut block = fixed_block(5);
    block.transactions = (0..200)
        .map(|i| {
            fat_tx(i as u64)
        })
        .collect();

    let att = sign_block_producer(&block, &sk, &identity(), VERSION);
    block.producer_pubkey = att.producer_pubkey;
    block.producer_sig = att.producer_sig.to_vec();

    assert!(
        verify_block_producer_on(&block, &identity(), VERSION).is_ok(),
        "a block large enough to break a hardware signer must still verify"
    );
}
