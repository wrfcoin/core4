//! The #2130 characterisations, **inverted at V2** — producer authority is now
//! enforced on blocks that came out of the real production path (#2226/#2290).
//!
//! # Why this is a separate test binary
//!
//! `block_producer_authority.rs` asserts it runs at the **V1 default**
//! (`v1_producer_emits_no_state_root_commitment` depends on it, because a V1
//! producer must not stamp a state-root commitment — that key would move every
//! frozen V1 block hash). `SigningProtocolVersion::install_active` is a
//! process-global `OnceLock` whose **first install wins**, so V2 cannot be
//! installed in that binary without breaking the V1 test. Hence a second
//! binary: same production path, V2 installed, negatives instead of
//! characterisations.
//!
//! # What this adds over the existing coverage
//!
//! `producer_auth_enforcement_e2e_2290.rs` already pins the gate itself, and
//! pins it well — signed member accepted, unsigned rejected, non-member
//! rejected, wrong payout rejected, unseeded chain unenforced. But every block
//! there is **hand-built** by a local `block_h1` helper that stamps
//! `producer_pubkey` and `producer_sig` itself.
//!
//! That leaves the question this file answers: does a block the **producer
//! actually built** authenticate? The production path has three ordering
//! constraints that a hand-built fixture cannot get wrong, because it does not
//! perform them at all:
//!
//!   1. `stamp_producer_pubkey` must run **before** the hash — `producer_pubkey`
//!      folds into `canonical_body`, so stamping it afterwards would leave the
//!      key outside the block's identity;
//!   2. `attach_producer_signature` must run **after** the hash — `producer_sig`
//!      is deliberately outside it (signing your own signature is impossible);
//!   3. the bytes signed must be the same bytes the verifier reconstructs, at
//!      the same `SigningProtocolVersion` and `ChainIdentity`.
//!
//! Get any of those wrong and every produced block is rejected by every peer,
//! while the hand-built fixtures stay green. That is the failure this file
//! exists to catch, and it is a launch-blocking one: the #2034 cut activates V2,
//! so V2 is the only version that will ever run in production.
//!
//! Run:  cargo test -p wrfcoin-shared-protocol --test block_producer_authority_v2

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use rustc_hash::FxHashSet;
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::producer_authority::verify_block_producer_authorized;
use wrfcoin_shared_protocol::consensus::producer_set::{ProducerMember, ProducerSetChange};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
use wrfcoin_shared_protocol::crypto::rotation::{RotationPlan, rotation_pop_message};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;
use wrfcoin_shared_protocol::mempool::pool::Mempool;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Install V2 and assert it is in force.
///
/// Idempotent and first-install-wins, so every test calls it and the assertion
/// is what makes the binary's premise checked rather than assumed: if anything
/// in this process installed V1 first, these tests would silently degrade into
/// the V1 characterisations they exist to invert.
fn v2() -> SigningProtocolVersion {
    let installed = SigningProtocolVersion::install_active(SigningProtocolVersion::V2);
    assert_eq!(
        installed,
        SigningProtocolVersion::V2,
        "this binary must run at V2 — something installed {installed} first, and \
         every assertion below would be about the wrong protocol version"
    );
    assert_eq!(SigningProtocolVersion::active(), SigningProtocolVersion::V2);
    installed
}

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// Canonical payout `Address` (0x-hex) — the coinbase form the mining route
/// accepts, NOT `pk.to_address()`'s wrf form (#2290 §R2).
fn payout_of(pk: &PublicKey) -> String {
    let key: [u8; 32] = pk.as_bytes().try_into().expect("ed25519 pubkey is 32 bytes");
    Address(key[0..20].try_into().expect("20-byte prefix")).to_string()
}

fn member_of(pk: &PublicKey, id: &str) -> ProducerMember {
    ProducerMember {
        producer_id: id.to_string(),
        payout_address: payout_of(pk),
        public_key: pk.as_bytes().try_into().expect("ed25519 pubkey is 32 bytes"),
    }
}

/// The genesis-seeded producer-set history a fresh-genesis chain carries.
fn seeded(members: Vec<ProducerMember>) -> Vec<ProducerSetChange> {
    vec![ProducerSetChange::new(0, 0, members)]
}

/// Build a block through the **production producer path** at V2, signed by
/// `signer_key`, paying `miner_address`.
///
/// Produced on a chain whose own `producer_set_history` is **empty**, so
/// `create_next_block`'s append-boundary gate is unenforced and production is
/// permitted for any key. That separation is deliberate: it lets a stranger's
/// block be *built* so the acceptance gate can be shown to reject it. A seeded
/// producing chain refusing to append is a different property, already covered
/// by `native-chain/tests/producer_auth_append_boundary_2290.rs`.
///
/// Also returns whether the block passes full production verification, so every
/// rejection below is attributable to producer authority rather than to a block
/// that was malformed at V2 in some unrelated way.
async fn produce_at_v2(signer_key: &PrivateKey, miner_address: &str) -> (Block, bool) {
    v2();
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();
    pop.set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
        signer_key.clone(),
        "TEST_PRODUCER_KEY",
    )));

    let chain = Blockchain::default();
    let tip = chain.tip;
    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    let block = pop
        .create_next_block(blockchain, mempool, miner_address)
        .await
        .expect("production producer path must build a block at V2");

    let oracle_set: FxHashSet<String> = FxHashSet::default();
    let chain_valid = pop
        .verify_block_chained(&block, &tip, &oracle_set, 0, &[])
        .await
        .expect("production verification must not error at V2");

    (block, chain_valid)
}

// ---------------------------------------------------------------------------
// The control: the production path signs something the gate accepts
// ---------------------------------------------------------------------------

/// A block built by the real producer, under a key the seeded set names, paying
/// that member's stable payout address, is **authorized**.
///
/// This is the control the two inversions below depend on, and it is also the
/// only test in the tree that proves the producer's own
/// stamp-hash-sign ordering agrees with the verifier's reconstruction at V2.
#[tokio::test]
async fn production_block_from_a_seeded_member_is_authorized_at_v2() {
    let (sk, pk) = keypair(2_226_001);
    let member = member_of(&pk, "genesis-producer");
    let history = seeded(vec![member.clone()]);

    let (block, chain_valid) = produce_at_v2(&sk, &member.payout_address).await;

    assert!(
        chain_valid,
        "precondition: the produced block must be valid on the ordinary \
         verification path, or a producer-authority verdict below says nothing"
    );
    assert_ne!(
        block.producer_pubkey, [0u8; 32],
        "the producer must have stamped its public key into the block — an \
         all-zero key means stamp_producer_pubkey did not run, and the gate \
         would reject every block this node ever produces"
    );
    assert!(
        !block.producer_sig.is_empty(),
        "the producer must have attached a signature"
    );
    assert_eq!(
        block.producer_pubkey.as_slice(),
        pk.as_bytes(),
        "the stamped key must be the signer's own"
    );

    assert_eq!(
        verify_block_producer_authorized(&block, &history),
        Ok(()),
        "a seeded member's block, built by the production path at V2, must be \
         authorized. Failure here means the bytes the producer signed are not \
         the bytes the verifier reconstructs — every block would be rejected by \
         every peer while the hand-built fixtures in \
         producer_auth_enforcement_e2e_2290.rs stay green"
    );
}

// ---------------------------------------------------------------------------
// Inversion 1 — was: "any producer identity at all is accepted"
// ---------------------------------------------------------------------------

/// **Now rejected.** `production_block_verification_accepts_an_entirely_unknown_producer`
/// recorded that no producer key authority was applied to blocks at all, in
/// either direction. Under a seeded set at V2 it is.
///
/// The stranger signs correctly and pays **its own** valid payout address, so
/// non-membership is the only available reason to reject — the assertion is not
/// riding on a malformed block or a payout mismatch.
#[tokio::test]
async fn production_block_from_an_entirely_unknown_producer_is_rejected_at_v2() {
    let (_, member_pk) = keypair(2_226_001);
    let (stranger_sk, stranger_pk) = keypair(2_130_999);
    let history = seeded(vec![member_of(&member_pk, "genesis-producer")]);

    let (block, chain_valid) = produce_at_v2(&stranger_sk, &payout_of(&stranger_pk)).await;

    assert!(
        chain_valid,
        "the stranger's block must be otherwise VALID — that is what makes the \
         rejection attributable to producer authority alone"
    );
    assert_eq!(
        block.producer_pubkey.as_slice(),
        stranger_pk.as_bytes(),
        "precondition: the block is genuinely signed by the stranger"
    );

    assert!(
        verify_block_producer_authorized(&block, &history).is_err(),
        "INVERTED (#2130 → #2290): a correctly-signed block from a producer the \
         seeded set does not name must be REJECTED. This assertion previously \
         read `accepted`."
    );
}

// ---------------------------------------------------------------------------
// Inversion 2 — was: "a revoked producer key is accepted"
// ---------------------------------------------------------------------------

/// **Now rejected.** `production_block_verification_accepts_a_revoked_producer_key`
/// recorded that the rotation plan considered the old key revoked at this height
/// and production verification accepted a block produced under it anyway,
/// "because it never asks."
///
/// It asks now — though not via `RotationPlan`, which is the part worth being
/// precise about. Authority is resolved from the on-chain producer set, so a
/// rotated-away key is rejected because **the set names the new key and not the
/// old one**, not because a rotation plan was consulted at verification time.
/// The rotation plan is retained here only to establish that the old key really
/// is the revoked one, so the test still describes the #2130 scenario rather
/// than a generic non-member.
///
/// That distinction is the honest answer to the three #2241 review rounds that
/// asked for `RotationPlan::accepts_key_at` to be wired into block
/// verification: the enforcement point is membership in `state`, which is
/// stronger — it lives in `state_root`, so it cannot be un-seeded to dodge the
/// gate, whereas a rotation plan is local configuration.
#[tokio::test]
async fn production_block_from_a_revoked_producer_key_is_rejected_at_v2() {
    const EFFECTIVE_HEIGHT: u64 = 1;

    let (old_sk, old_pk) = keypair(2_130_001);
    let (new_sk, new_pk) = keypair(2_130_002);

    // A structurally valid rotation with a real proof of possession, so nothing
    // here passes because the plan was malformed.
    let plan = RotationPlan {
        old: old_pk.clone(),
        new: new_pk.clone(),
        effective_height: EFFECTIVE_HEIGHT,
        overlap_blocks: 16,
        proof: Some(new_sk.sign(&rotation_pop_message(&old_pk, &new_pk, EFFECTIVE_HEIGHT))),
    };

    // The set carries the POST-rotation key, which is what a chain that honoured
    // the rotation would hold. Payout is stable across rotation (§R2), so it
    // stays keyed to the member identity, not to the signing key.
    let member = member_of(&new_pk, "rotated-producer");
    let history = seeded(vec![member.clone()]);

    // Produced under the OLD, revoked key, paying the member's stable address —
    // so payout is correct and the revoked signing key is the only defect.
    let (block, chain_valid) = produce_at_v2(&old_sk, &member.payout_address).await;

    assert!(
        !plan.accepts_key_at(&old_pk, block.height),
        "precondition: the rotation plan must consider the old key revoked at \
         height {}",
        block.height
    );
    assert!(
        chain_valid,
        "the block must be otherwise valid, so the rejection is attributable to \
         the revoked producer key"
    );
    assert_eq!(
        block.producer_pubkey.as_slice(),
        old_pk.as_bytes(),
        "precondition: the block is genuinely signed by the revoked key"
    );

    assert!(
        verify_block_producer_authorized(&block, &history).is_err(),
        "INVERTED (#2130 → #2290): a block produced under a key the chain has \
         rotated away from must be REJECTED. This assertion previously read \
         `accepted`, with the note 'when block producer authentication lands, \
         invert this assertion'."
    );

    // And the complement, so the rejection above is about the KEY and not about
    // this member or this payout address: the same member, same payout, signed
    // by the new key, is accepted.
    let (rotated_block, _) = produce_at_v2(&new_sk, &member.payout_address).await;
    assert_eq!(
        verify_block_producer_authorized(&rotated_block, &history),
        Ok(()),
        "the post-rotation key must be accepted for the same member and payout — \
         otherwise the rejection above could be about membership or payout \
         rather than about the revoked key"
    );
}

// ---------------------------------------------------------------------------
// The activation boundary, asserted rather than assumed
// ---------------------------------------------------------------------------

/// The SAME produced block that the seeded set rejects is accepted on a chain
/// that never configured a producer set.
///
/// This is the activation semantics — enforce **iff** seeded — and it is the
/// reason the two inversions above are not simply "the gate now rejects
/// everything". It is also why the pre-cut chain and the in-process test
/// binaries are unaffected: they carry no set. The set lives in `state_root`, so
/// a configured chain cannot be un-seeded to reach this branch.
#[tokio::test]
async fn the_same_production_block_is_unenforced_on_an_unseeded_chain_at_v2() {
    let (stranger_sk, stranger_pk) = keypair(2_130_999);
    let (_, member_pk) = keypair(2_226_001);

    let (block, _) = produce_at_v2(&stranger_sk, &payout_of(&stranger_pk)).await;

    assert!(
        verify_block_producer_authorized(
            &block,
            &seeded(vec![member_of(&member_pk, "genesis-producer")])
        )
        .is_err(),
        "precondition: this block IS rejected under a seeded set"
    );
    assert_eq!(
        verify_block_producer_authorized(&block, &[]),
        Ok(()),
        "…and accepted with no producer set configured — enforcement is \
         activated by seeding, so legacy and dev chains are not bricked"
    );
}

// ---------------------------------------------------------------------------
// Why the signature is worth anything: the key is inside the block's identity
// ---------------------------------------------------------------------------

/// `producer_pubkey` folds into `canonical_body`, so at V2 it is part of the
/// block hash. Swapping the claimed producer key therefore **moves the block's
/// identity**, which is what stops a valid signature being re-pointed at a
/// different claimed producer.
///
/// Asserted on a block from the production path, because this is the property
/// that makes the stamp-before-hash ordering in `create_next_block` load-bearing
/// rather than incidental.
#[tokio::test]
async fn producer_pubkey_is_bound_into_the_v2_block_hash() {
    let (sk, pk) = keypair(2_226_001);
    let (_, other_pk) = keypair(2_226_002);

    let (block, _) = produce_at_v2(&sk, &payout_of(&pk)).await;
    let sealed = block.hash.expect("a produced block is hashed");
    assert_eq!(
        block.calculate_hash(),
        sealed,
        "precondition: the sealed hash must match a recomputation, or the \
         comparison below is against a stale value"
    );

    let mut repointed = block.clone();
    repointed.producer_pubkey = other_pk
        .as_bytes()
        .try_into()
        .expect("ed25519 pubkey is 32 bytes");

    assert_ne!(
        repointed.calculate_hash(),
        sealed,
        "swapping producer_pubkey must move the V2 block hash — if it did not, \
         the key would sit outside the block's identity and a harvested \
         signature could be re-pointed at a different claimed producer (#2226)"
    );
}
