//! Block-producer authentication seam — #2290 / #2220 / #2226.
//!
//! # What this is
//!
//! Today a block carries **no producer signature and no producer public key**
//! (`docs/security/PROTOCOL_SPEC.md` §3 divergence D3, filed as #2220). Producer
//! authority exists only as an HTTP Operator-token gate on `/blocks/mine`; a
//! gossiped or IBD block is accepted with *zero* producer authentication, and
//! key rotation cannot be enforced on the block path
//! (`shared-protocol/tests/block_producer_authority.rs` characterises this).
//!
//! This module is the **verifier + signer + authority-predicate seam** that
//! #2290 needs, built ahead of the block-format change so the property can be
//! tested first. It is the design of the two merged security docs made
//! executable:
//!
//! - `docs/security/BLOCK_PRODUCER_AUTHENTICATION.md` (#2291) — the authority on
//!   what #2290 must do (preimage §3.3, per-engine predicate §5, snapshot bound §4).
//! - `docs/security/GENESIS_CUT_PLAN.md` (#2293) — how it rides the #2034 cut
//!   (composed frame §3.1, block-preimage reconciliation §3.2, ordering §4).
//!
//! # Status — #2226 wired the seam onto the real block
//!
//! The block-format mutation is now on `Block` (#2226, riding #2219): `Block`
//! carries `producer_pubkey: [u8; 32]` and `producer_sig: Vec<u8>`,
//! `<Block as DomainSeparated>::canonical_body` folds `producer_pubkey` in, and
//! `Block::calculate_hash` at V2 is `hash_bytes(signing_preimage())` — one block
//! encoding (`GENESIS_CUT_PLAN.md` §3.2). So:
//!
//! - [`producer_canonical_body`] no longer keeps a hand replica; it **delegates**
//!   to `Block::canonical_body`, so the seam preimage and the block identity are
//!   one construction. `producer_pubkey` is therefore in **both** the hash and
//!   the signature by construction — re-attributing a block changes its hash
//!   (`seam_preimage_equals_block_signing_preimage_2226`,
//!   `attribution_swap_on_real_block_fields_is_rejected_2226`).
//! - [`ProducerAttestation::from_block`] reads the attestation off the real
//!   fields, and [`verify_block_producer_on`] is the live verifier over the
//!   block's own preimage.
//! - The golden-vector test (`tests/block_producer_signing.rs`) still pins the
//!   bytes and locks cross-repo signer parity (`GENESIS_CUT_PLAN.md` §3.4/§7).
//!
//! **Remaining #2290 follow-up:** calling [`verify_block_producer_on`] from the
//! live `verify_block` paths 1 (gossip) and 2 (IBD) with the per-engine authority
//! predicates, and signing in `create_next_block` (node-signs, `load_block_signer`).
//!
//! # The preimage — no new tag
//!
//! The producer signature is bound under the **existing** `wrfcoin.block_header.v1`
//! tag (`GENESIS_CUT_PLAN.md` §3.2: "no new BlockProducer tag"), through the
//! `#2123/#2232` chain-bound frame (`domain_preimage_bound`). `producer_pubkey`
//! joins the signed body **after** the timestamp and **before** the transaction
//! sequence; `producer_sig` is outside the preimage — a signature cannot commit
//! to itself.

use crate::blockchain::block::Block;
use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion, domain_preimage_bound};
use crate::crypto::domain_impls::BodyBuilder;
use crate::crypto::domain_signing::DomainSeparated;
use crate::crypto::rotation::RotationPlan;
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};
use thiserror::Error;

/// The domain tag the producer signature is bound under — the **same** tag as
/// the existing `impl DomainSeparated for Block`, sourced from it directly so a
/// rename cannot silently split the two (#2291 §3.3 / #2293 §3.2).
pub const BLOCK_PRODUCER_DOMAIN_TAG: &[u8] = <Block as DomainSeparated>::DOMAIN_TAG;

/// Why a block's producer failed authentication.
///
/// Authentication only — *authority* (is this key allowed to produce here?) is a
/// separate decision each engine makes via the `authorize_producer_*` predicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum ProducerAuthError {
    /// No producer signature present (the all-zero attestation an unsigned block
    /// carries).
    #[error("block carries no producer signature")]
    Unsigned,
    /// `producer_pubkey` is not a valid ed25519 point.
    #[error("producer public key is malformed")]
    MalformedKey,
    /// The signature does not verify (`verify_strict`, #2043) under the declared
    /// producer key — includes the re-attribution attack: a valid block re-signed
    /// under a different key does not verify against the swapped-in key.
    #[error("producer signature does not verify under the declared producer key")]
    BadSignature,
}

/// The two fields #2290 will add to [`Block`], modeled standalone so the verifier,
/// its negative tests and the golden vectors can exist before the `Block` struct
/// is mutated (that mutation lands inside #2226, after #2219 — see module docs).
///
/// Fixed-width per `BLOCK_PRODUCER_AUTHENTICATION.md` §6.3: `+96 bytes/block`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProducerAttestation {
    /// ed25519 public key of the producer. Inside the hash **and** the signing
    /// preimage.
    pub producer_pubkey: [u8; 32],
    /// ed25519 signature over [`block_producer_signing_digest`] — the 32-byte
    /// SHA3-256 of [`block_producer_preimage`], **not** the preimage itself
    /// (#2728: a hardware signer refuses inputs above ~1 KiB). Inside **neither**
    /// the hash nor the preimage.
    pub producer_sig: [u8; 64],
}

impl ProducerAttestation {
    /// The all-zero attestation an unsigned block carries.
    pub fn unsigned() -> Self {
        Self {
            producer_pubkey: [0u8; 32],
            producer_sig: [0u8; 64],
        }
    }

    /// Whether this attestation carries no signature at all.
    pub fn is_unsigned(&self) -> bool {
        self.producer_sig == [0u8; 64]
    }

    /// Read the attestation a #2226 [`Block`] carries on its own
    /// `producer_pubkey` / `producer_sig` fields.
    ///
    /// This is the verify-boundary conversion `block.rs` documents:
    /// `Block::producer_sig` is a `Vec<u8>` (serde has no fixed `[u8; 64]` impl)
    /// holding 64 bytes when signed and empty when unsigned. A signature that is
    /// not exactly 64 bytes yields the all-zero (unsigned) attestation, which
    /// [`verify_block_producer`] rejects as [`ProducerAuthError::Unsigned`].
    pub fn from_block(block: &Block) -> Self {
        let producer_sig = <[u8; 64]>::try_from(block.producer_sig.as_slice()).unwrap_or([0u8; 64]);
        Self {
            producer_pubkey: block.producer_pubkey,
            producer_sig,
        }
    }
}

/// Canonical body for a producer-authenticated block.
///
/// **#2226 wired the seam in.** `producer_pubkey` now lives on `Block` and
/// `<Block as DomainSeparated>::canonical_body` folds it into the body (after the
/// timestamp, before the tx list). This function therefore no longer maintains a
/// hand-kept replica — it **delegates** to `Block::canonical_body`, so the seam
/// and block identity share exactly one encoding and cannot drift. For the
/// block's own key it is a direct call; for a *different* key (the
/// attribution-swap negative test) it evaluates the body the block *would* have
/// under that key. The golden-vector test still pins the bytes.
fn producer_canonical_body(block: &Block, producer_pubkey: &[u8; 32]) -> Vec<u8> {
    if &block.producer_pubkey == producer_pubkey {
        <Block as DomainSeparated>::canonical_body(block)
    } else {
        let mut probe = block.clone();
        probe.producer_pubkey = *producer_pubkey;
        <Block as DomainSeparated>::canonical_body(&probe)
    }
}

/// The domain-separated preimage the producer signs and the verifier checks.
///
/// Framed under the active-chain identity + protocol version (#2123/#2232 V2
/// frame): `len(tag) || tag || len(chain_id) || chain_id || len(network_id) ||
/// network_id || protocol_version || len(body) || body`.
pub fn block_producer_preimage(
    block: &Block,
    producer_pubkey: &[u8; 32],
    identity: &ChainIdentity,
    version: SigningProtocolVersion,
) -> Vec<u8> {
    domain_preimage_bound(
        BLOCK_PRODUCER_DOMAIN_TAG,
        identity,
        version,
        &producer_canonical_body(block, producer_pubkey),
    )
}

/// The exact bytes that get signed: a **fixed-size digest** of the producer
/// preimage, never the preimage itself.
///
/// # Why this exists (core4 #2728)
///
/// [`block_producer_preimage`] serialises the **whole block, transactions
/// included**, so its length grows without bound with the mempool. Ed25519 is
/// defined over arbitrary-length messages, so a *software* signer signs that
/// happily — which is why every test, every CI job and every dev node passed.
///
/// A *hardware* signer does not. Measured 2026-08-05 against the live producer
/// YubiKey (serial `35278470`, PIV 9A, ED25519) through `libykcs11.so`:
///
/// | preimage | `C_Sign` |
/// |---|---|
/// | 1024 B | OK |
/// | 4096 B | **`CKR_DATA_LEN_RANGE`** |
///
/// The node failed producing block 1 with **three** transactions: the first real
/// block already exceeded the card's input limit, so an HSM-backed producer
/// could not sign *any* non-trivial block. Hashing first makes the signed input
/// 32 bytes regardless of block size.
///
/// # This is consensus, not a local detail
///
/// Signing a digest changes what a signature commits to, so **every** call site
/// must use this function — the in-process path, the HSM path, and the verifier.
/// A partial change produces blocks that peers reject with no useful error.
/// They are deliberately routed through one helper for that reason.
///
/// SHA3-256 matches the hash used elsewhere in the protocol
/// (`Block::calculate_hash` at V2 is `hash_bytes(signing_preimage())`), so this
/// introduces no new primitive. The preimage is already domain-separated by
/// [`domain_preimage_bound`], so hashing it does not weaken domain separation:
/// distinct domains still yield distinct digests.
pub fn block_producer_signing_digest(
    block: &Block,
    producer_pubkey: &[u8; 32],
    identity: &ChainIdentity,
    version: SigningProtocolVersion,
) -> [u8; 32] {
    let preimage = block_producer_preimage(block, producer_pubkey, identity, version);
    *crate::crypto::hash::hash_bytes(&preimage).as_bytes()
}

/// Produce a producer attestation over `block` using `signer` as the producer
/// key.
///
/// The **signing** side of the seam. Un-wired: `create_next_block` does not call
/// this yet — that lands with #2226. In production the private key is held by the
/// node's `crypto::signer::BlockSigner` (`load_block_signer`, HSM-backed); this
/// `PrivateKey` form is the in-process / test path.
pub fn sign_block_producer(
    block: &Block,
    signer: &PrivateKey,
    identity: &ChainIdentity,
    version: SigningProtocolVersion,
) -> ProducerAttestation {
    let pubkey = PublicKey::from_private_key(signer);
    let producer_pubkey: [u8; 32] = pubkey
        .as_bytes()
        .try_into()
        .expect("ed25519 public key is 32 bytes");
    // #2728: sign a fixed-size digest, never the unbounded preimage.
    let digest = block_producer_signing_digest(block, &producer_pubkey, identity, version);
    let sig = signer.sign(&digest);
    ProducerAttestation {
        producer_pubkey,
        producer_sig: sig.as_bytes(),
    }
}

/// Produce a producer attestation using an HSM-capable [`BlockSigner`] — the
/// **production** signing path (#2290 P4), as opposed to the [`sign_block_producer`]
/// `&PrivateKey` form used in-process and by tests.
///
/// This is what the live block builders call: the key is held by the node's
/// `crypto::signer::BlockSigner` (`load_block_signer`, HSM-backed in release). It
/// **reuses [`block_producer_signing_digest`]** — it does not re-derive anything —
/// so the bytes signed here are byte-identical to what [`verify_block_producer_on`]
/// re-derives and checks. Those bytes are the 32-byte digest of the preimage, not
/// the preimage (#2728).
///
/// The caller must have already set `block.producer_pubkey` to this signer's key
/// **before hashing** (`producer_pubkey` is in the block hash); the returned
/// attestation carries that same key so the caller can assert consistency.
///
/// Signing goes through [`BlockSigner::sign_at_height`](crate::crypto::signer::BlockSigner::sign_at_height)
/// at `block.height`, never the heightless `sign` (#2290 P1-2). `BlockSigner`
/// states that callers holding a height MUST use the height-aware form, and a
/// block always has one. This is not a preference: `load_block_signer` wraps
/// every production signer in
/// [`FailClosedRemoteSigner`](crate::crypto::rotation::FailClosedRemoteSigner),
/// which **refuses** heightless signing while a validator key rotation is active
/// — so the heightless call both lost rotation enforcement on the block path and
/// made a rotating node unable to produce blocks at all. Passing the height
/// restores the boundary check at the point of signing: a rotated-out key is
/// refused here rather than reaching a real block.
///
/// Fallible because a hardware signer can fail (token unavailable, PIN, I/O) and
/// because the rotation boundary can refuse. The caller should fail block
/// production rather than emit an unsigned block that peers will reject — hence
/// `anyhow::Result`, distinct from the authentication [`ProducerAuthError`]
/// domain used on the verify side.
pub fn attest_block_producer(
    block: &Block,
    signer: &dyn crate::crypto::signer::BlockSigner,
    identity: &ChainIdentity,
    version: SigningProtocolVersion,
) -> anyhow::Result<ProducerAttestation> {
    let pubkey = signer.public_key();
    let producer_pubkey: [u8; 32] = pubkey
        .as_bytes()
        .try_into()
        .map_err(|_| anyhow::anyhow!("producer signer public key is not 32 bytes"))?;
    // #2728: a YubiKey refuses `C_Sign` above ~1 KiB (CKR_DATA_LEN_RANGE), and
    // the preimage grows with the block's transaction count. Sign the digest.
    let digest = block_producer_signing_digest(block, &producer_pubkey, identity, version);
    let signature = signer.sign_at_height(&digest, block.height)?;
    Ok(ProducerAttestation {
        producer_pubkey,
        producer_sig: signature.as_bytes(),
    })
}

/// Authenticate a block's producer: verify `producer_sig` over the
/// domain-separated preimage under `producer_pubkey`, returning the authenticated
/// key.
///
/// This is authentication, **not authority**. It answers "did the holder of
/// `producer_pubkey` sign this exact block?" — it does not decide whether that
/// key is *allowed* to produce at this height. Each consensus engine applies its
/// own predicate ([`authorize_producer_pop`] / [`authorize_producer_in_set`] /
/// [`authorize_producer_rotation`]) to the returned key. A single shared verifier
/// called by paths 1 and 2 is what stops those call sites drifting apart
/// (`BLOCK_PRODUCER_AUTHENTICATION.md` §5).
///
/// Not yet called from the live `verify_block` — that wiring is #2226.
pub fn verify_block_producer(
    block: &Block,
    attestation: &ProducerAttestation,
    identity: &ChainIdentity,
    version: SigningProtocolVersion,
) -> Result<PublicKey, ProducerAuthError> {
    if attestation.is_unsigned() {
        return Err(ProducerAuthError::Unsigned);
    }
    let pubkey = PublicKey::from_bytes(&attestation.producer_pubkey)
        .map_err(|_| ProducerAuthError::MalformedKey)?;
    let signature = Signature::from_bytes(&attestation.producer_sig)
        .map_err(|_| ProducerAuthError::BadSignature)?;
    // #2728: must hash exactly as the signers do, or nothing verifies.
    let digest =
        block_producer_signing_digest(block, &attestation.producer_pubkey, identity, version);
    if pubkey.verify(&digest, &signature) {
        Ok(pubkey)
    } else {
        Err(ProducerAuthError::BadSignature)
    }
}

/// Authenticate the producer of a #2226 [`Block`] that carries its own
/// `producer_pubkey` / `producer_sig`.
///
/// This is the live entrypoint after #2226: it reads the attestation off the
/// block and verifies it over the **same** preimage the block hash is derived
/// from (`Block::calculate_hash` at V2 is `hash_bytes(signing_preimage())`, and
/// `producer_pubkey` is folded into that body). So the identity that is *hashed*
/// and the identity that is *signed* are one and the same — re-attributing a
/// block to a different key changes its hash, which is the attribution-swap
/// closure §3.2 argues for, now true by construction rather than by a hand-kept
/// replica.
///
/// Returns the authenticated key; authority (is this key allowed to produce
/// here?) remains each engine's `authorize_producer_*` decision.
pub fn verify_block_producer_on(
    block: &Block,
    identity: &ChainIdentity,
    version: SigningProtocolVersion,
) -> Result<PublicKey, ProducerAuthError> {
    verify_block_producer(
        block,
        &ProducerAttestation::from_block(block),
        identity,
        version,
    )
}

// ---------------------------------------------------------------------------
// Per-engine authority predicates (§5). Applied to the key
// `verify_block_producer` returns. Un-wired — the engines will call these once
// the block carries an attestation (#2226).
// ---------------------------------------------------------------------------

/// **PoP** (testnet): the producer key must derive the block's coinbase
/// recipient address. PoP records the producer *only* as that address, so
/// binding the authenticated key to it is what turns an unauthenticated claim
/// into an authenticated one.
pub fn authorize_producer_pop(key: &PublicKey, coinbase_recipient: &str) -> bool {
    key.to_address() == coinbase_recipient
}

/// **PoS / DPoS / HotStuff / Raft**: the producer key must be in the active
/// validator/delegate set. The set is modeled here as authorized producer keys;
/// each engine maps its own set (e.g. `validators_from_state` in
/// `consensus-engines`) to keys before calling. Checking the *key*'s membership
/// — not the self-declared `metadata["validator"]` string — is the whole point.
pub fn authorize_producer_in_set(key: &PublicKey, authorized_keys: &[PublicKey]) -> bool {
    authorized_keys.iter().any(|k| k == key)
}

/// **Rotation** on the block path: the producer key must be the one the rotation
/// plan considers authoritative at this height (#2241 `RotationPlan::accepts_key_at`).
/// A rotated-out key fails here even though its signature authenticates.
pub fn authorize_producer_rotation(key: &PublicKey, height: u64, plan: &RotationPlan) -> bool {
    plan.accepts_key_at(key, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::Transaction;
    use crate::crypto::hash::Hash;
    use chrono::{DateTime, Utc};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn identity() -> ChainIdentity {
        ChainIdentity::new("wrfcoin-testnet", "testnet")
    }

    fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
        let mut rng = StdRng::seed_from_u64(seed);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        (sk, pk)
    }

    /// A block with a fixed timestamp so preimages are deterministic.
    fn fixed_block() -> Block {
        let mut b = Block::new(3, Hash::default(), Vec::<Transaction>::new(), 1, 7);
        b.timestamp = DateTime::<Utc>::from_timestamp_millis(1_700_000_000_000).unwrap();
        b
    }

    #[test]
    fn tag_matches_the_block_domain_tag() {
        assert_eq!(BLOCK_PRODUCER_DOMAIN_TAG, b"wrfcoin.block_header.v1");
    }

    #[test]
    fn sign_then_verify_round_trips_and_returns_the_producer_key() {
        let (sk, pk) = keypair(1);
        let block = fixed_block();
        let att = sign_block_producer(&block, &sk, &identity(), SigningProtocolVersion::V2);
        let got = verify_block_producer(&block, &att, &identity(), SigningProtocolVersion::V2)
            .expect("honest attestation verifies");
        assert_eq!(got, pk);
    }

    // ------------------------------------------------------------------
    // #2290 P1-2 — the producer path is rotation-aware.
    // ------------------------------------------------------------------

    /// Wrap `sk` in the rotation-enforcing signer with `plan` attached, exactly
    /// as `load_block_signer` does in production (#2130).
    #[cfg(debug_assertions)]
    fn rotating_signer(
        sk: PrivateKey,
        plan: crate::crypto::rotation::RotationPlan,
    ) -> crate::crypto::rotation::FailClosedRemoteSigner {
        use crate::crypto::signer::EnvKeySigner;
        crate::crypto::rotation::FailClosedRemoteSigner::new(std::sync::Arc::new(
            EnvKeySigner::from_private_key(sk, "TEST_PRODUCER_KEY"),
        ))
        .with_rotation_plan(plan)
    }

    /// #2290 P1-2 (Elsa CHANGES, round 1): **block production must survive an
    /// active validator key rotation.**
    ///
    /// `attest_block_producer` used the heightless `BlockSigner::sign` even
    /// though `block.height` was right there. `FailClosedRemoteSigner` — which
    /// `load_block_signer` wraps every production signer in — deliberately
    /// *refuses* heightless signing while a rotation plan is active, because it
    /// cannot tell whether its key is authoritative for a payload with no
    /// height. So a node that merely had a rotation configured could not
    /// produce a block at all: every attempt failed with "heightless signing
    /// refused during rotation", which is exactly what Elsa's probe hit.
    ///
    /// Here the producer's key IS authoritative at the block's height (the
    /// block sits before the overlap window, where the old key still rules), so
    /// signing must succeed and the result must verify. RED before the fix: the
    /// heightless refusal makes `attest_block_producer` return `Err`.
    #[cfg(debug_assertions)]
    #[test]
    fn producer_signs_at_block_height_during_an_active_rotation_2290_p1_2() {
        use crate::crypto::rotation::RotationPlan;

        let (sk, pk) = keypair(11);
        let (_other_sk, new_key) = keypair(12);
        let block = fixed_block(); // height 7

        // Rotation to `new_key` at height 100, overlap 10 ⇒ the overlap window
        // opens at 90. At height 7 the OLD key (ours) is the authoritative one.
        let plan = RotationPlan {
            old: pk.clone(),
            new: new_key,
            effective_height: 100,
            overlap_blocks: 10,
            proof: None,
        };
        assert!(
            plan.accepts_key_at(&pk, block.height),
            "test premise: the producer key must be authoritative at this height"
        );

        let signer = rotating_signer(sk, plan);
        let att = attest_block_producer(&block, &signer, &identity(), SigningProtocolVersion::V2)
            .expect(
                "a producer whose key is authoritative at this height must be able to sign \
                 during an active rotation — the heightless path refuses mid-rotation, so \
                 this is RED unless the producer calls sign_at_height (#2290 P1-2)",
            );

        assert_eq!(
            att.producer_pubkey,
            <[u8; 32]>::try_from(pk.as_bytes()).unwrap(),
            "the attestation must carry the rotation-wrapped signer's active key"
        );
        let got = verify_block_producer(&block, &att, &identity(), SigningProtocolVersion::V2)
            .expect("a signature produced at the block height must verify");
        assert_eq!(got, pk);
    }

    /// The other half of the rotation guard: a key that is **not** authoritative
    /// at the block's height must be refused *at signing time*, with the
    /// rotation boundary named.
    ///
    /// This is the enforcement the heightless path silently skipped. RED before
    /// the fix for a subtler reason than its sibling: production did fail, but
    /// with the generic "refusing to sign without a height" message, not because
    /// the rotation boundary was evaluated. Asserting on the boundary reason is
    /// what makes this test load-bearing rather than vacuously green.
    #[cfg(debug_assertions)]
    #[test]
    fn producer_is_refused_when_its_key_is_not_authoritative_at_the_height_2290_p1_2() {
        use crate::crypto::rotation::RotationPlan;

        let (sk, pk) = keypair(21);
        let (_old_sk, old_key) = keypair(22);
        let (_new_sk, new_key) = keypair(23);
        let block = fixed_block(); // height 7

        // The rotation took effect at height 5 ⇒ from 5 onwards ONLY `new_key`
        // may sign. Our key is neither half of the plan: it is rotated out.
        let plan = RotationPlan {
            old: old_key,
            new: new_key,
            effective_height: 5,
            overlap_blocks: 2,
            proof: None,
        };
        assert!(
            !plan.accepts_key_at(&pk, block.height),
            "test premise: the producer key must NOT be authoritative at this height"
        );

        let signer = rotating_signer(sk, plan);
        let err = attest_block_producer(&block, &signer, &identity(), SigningProtocolVersion::V2)
            .expect_err("a rotated-out key must not be able to sign a block");
        let msg = err.to_string();
        assert!(
            msg.contains("not authoritative at height 7"),
            "the refusal must come from evaluating the rotation boundary at the block's \
             height — a generic heightless refusal would mean rotation is still not \
             enforced on the producer path (#2290 P1-2), got: {msg}"
        );
    }
}
