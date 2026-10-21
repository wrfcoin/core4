//! Where key authority at a height is — and is not — consulted (#2130).
//!
//! Three review rounds on #2241 asked for the same thing in escalating terms:
//! wire `RotationPlan::accepts_key_at` into the path a peer takes when it
//! accepts a block, and prove a revoked key is rejected there.
//!
//! That cannot be done, and these tests are the reason, stated executably
//! rather than in prose. **`Block` carries no producer signature and no
//! producer public key.** Under PoP the only record of who produced a block is
//! the coinbase recipient — an address, bound to no key by anything. Production
//! block verification therefore has no producer key to consult; it is not that
//! it has one and forgets to check it. There is no seam to wire a rotation plan
//! into. The seam has to be built first, and building it changes the block
//! format.
//!
//! # STATUS: the premise above is HISTORICAL (#2226/#2290 landed)
//!
//! `Block` now carries `producer_pubkey: [u8; 32]` and `producer_sig: Vec<u8>`
//! (#2371), and producer authority IS enforced — at the untrusted acceptance
//! boundaries (`pop_adapter::validate_block`, `p2p::sync::validate_block`), on
//! the production path (`/blocks/mine`), at `create_next_block`'s append
//! boundary, and as a persisted-boot backstop — all through the one shared
//! `verify_block_producer_authorized`.
//!
//! **The two `accepts_...` tests below are still correct, and they are NOT the
//! current state of enforcement.** They pass for a reason worth stating
//! precisely, because it is easy to misread them as "authority is unenforced":
//! `produce_and_verify` calls `verify_block_chained`, which is **not** one of the
//! gated boundaries — it never receives `producer_set_history`, so it has no set
//! to resolve authority against. They therefore characterise the *ungated*
//! verification helper at the V1 default, not the chain.
//!
//! The inversions the original notes asked for live in
//! **`block_producer_authority_v2.rs`**, which installs V2 and runs the same
//! production producer path against a seeded set: unknown producer REJECTED,
//! revoked key REJECTED, seeded member accepted, unseeded chain unenforced. They
//! are a separate binary because `install_active` is a process-global `OnceLock`
//! and `v1_producer_emits_no_state_root_commitment` below pins this binary to V1.
//!
//! Kept rather than deleted: V1 block hashes are frozen, so V1 behaviour remains
//! a real property of the shipped code until the #2034 cut activates V2.
//!
//! Run:
//!   cargo test -p wrfcoin-shared-protocol --test block_producer_authority
//!   cargo test -p wrfcoin-shared-protocol --test block_producer_authority_v2

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use rustc_hash::FxHashSet;
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::rotation::{RotationPlan, rotation_pop_message};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// Effective at height 1, so the very first block a fixture chain can produce is
/// already at/after the rotation boundary and the old key is revoked there.
const EFFECTIVE_HEIGHT: u64 = 1;
const OVERLAP: u64 = 16;

/// A structurally valid rotation from `old` to `new`, with a real proof of
/// possession so nothing here passes because the plan was malformed.
fn rotation_plan() -> (RotationPlan, PublicKey, PublicKey) {
    let (_, old) = keypair(2_130_001);
    let (new_sk, new) = keypair(2_130_002);
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

/// Produce a block through the **production** producer path and verify it
/// through the **production** verification path, exactly as a peer would.
///
/// `producer` is the miner address — under PoP, the entirety of the block's
/// record of who made it.
async fn produce_and_verify(producer: &str) -> (Block, bool) {
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();
    let chain = Blockchain::default();
    let tip = chain.tip;
    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    let block = pop
        .create_next_block(blockchain, mempool, producer)
        .await
        .expect("production producer path must build a block");

    let oracle_set: FxHashSet<String> = FxHashSet::default();
    let accepted = pop
        .verify_block_chained(&block, &tip, &oracle_set, 0, &[])
        .await
        .expect("production verification must not error");

    (block, accepted)
}

// ---------------------------------------------------------------------------
// "Can a peer still accept a block signed with a revoked key?"
// ---------------------------------------------------------------------------

/// **Through `verify_block_chained`, yes — and that is not a gap.** This helper
/// is not a gated acceptance boundary: it takes no `producer_set_history`, so it
/// has no authority to consult. Authority is enforced by
/// `verify_block_producer_authorized` at the boundaries that do have chain state.
///
/// Inverted at V2 in `block_producer_authority_v2.rs`:
/// `production_block_from_a_revoked_producer_key_is_rejected_at_v2`.
#[tokio::test]
async fn production_block_verification_accepts_a_revoked_producer_key() {
    let (plan, old, _new) = rotation_plan();

    let (block, accepted) = produce_and_verify(&old.to_string()).await;

    assert!(
        !plan.accepts_key_at(&old, block.height),
        "precondition: the plan must consider the old key revoked at height {}",
        block.height
    );
    assert!(
        accepted,
        "CHARACTERISATION (#2130): production block verification accepts a block \
         produced under a key the rotation plan has revoked at that height. \
         is not a gated boundary. INVERTED at V2 in \
         block_producer_authority_v2.rs (#2226/#2290)."
    );
}

/// Isolates the same fact for a key with no relationship to any rotation plan,
/// validator set, or registered identity — so what this pins is that
/// `verify_block_chained` applies no producer authority *at all*, rather than
/// that revocation specifically is unenforced.
///
/// That is still true of this helper and no longer true of the chain. Inverted
/// at V2 in `block_producer_authority_v2.rs`:
/// `production_block_from_an_entirely_unknown_producer_is_rejected_at_v2`.
#[tokio::test]
async fn production_block_verification_accepts_an_entirely_unknown_producer() {
    let (_, stranger) = keypair(2_130_999);

    let (_, accepted) = produce_and_verify(&stranger.to_string()).await;

    assert!(
        accepted,
        "CHARACTERISATION (#2130): verify_block_chained applies no producer \
         authority. INVERTED at V2 in block_producer_authority_v2.rs."
    );
}

/// This began as the tripwire under the two characterisations above: *"there is
/// nothing on a `Block` that could carry a producer signature, so there is
/// nothing for a verifier to check — fails the moment a signature-bearing field
/// is added."*
///
/// **It never fired, and it should have.** #2371 added `producer_pubkey: [u8;32]`
/// and `producer_sig: Vec<u8>` to `Block` and merged. The guard banned any field
/// name *containing* `signature`, `signer`, `producer_key`, `public_key` or
/// `proposer` — and `producer_pubkey` contains none of them (it is not
/// `producer_key`, not `public_key`), nor does `producer_sig` contain
/// `signature`. The list missed the real names by a few characters, so the
/// signal it existed to raise was never raised: a gate-shaped thing rather than
/// a gate, the same class as #2453/#2467's name-based blind spot.
///
/// Inverted to match the tree. The fields exist, so this now asserts they are
/// PRESENT — which is the assertion that keeps working: it fails if producer
/// authentication is ever removed or renamed out from under the verifier, and it
/// cannot be defeated by a near-miss substring.
///
/// The lesson is deliberately recorded in code rather than in a commit message:
/// a tripwire keyed on *absence* by fuzzy name matching cannot be trusted, and
/// this one silently protected nothing for the entire window in which the block
/// frame changed underneath it.
#[test]
fn block_carries_the_fields_producer_authentication_needs() {
    let block = Block::new(1, Default::default(), Vec::new(), 1, 1);
    let json = serde_json::to_value(&block).expect("Block serialises");
    let fields: Vec<String> = json
        .as_object()
        .expect("Block is a struct")
        .keys()
        .cloned()
        .collect();

    // Exact names, not substrings — the failure mode above was fuzzy matching.
    for required in ["producer_pubkey", "producer_sig"] {
        assert!(
            fields.iter().any(|f| f == required),
            "Block lost `{required}` ({fields:?}). Producer authentication is a \
             block-format property: without both fields there is nothing for \
             `verify_block_producer_authorized` to check, and every producer-auth \
             gate downstream silently degrades to accepting anything (#2226/#2290)."
        );
    }

    // The original guard, kept honest: assert it would NOT have caught these
    // names, so the near-miss is a checked fact rather than a claim in a comment.
    let legacy_banned = ["signature", "signer", "producer_key", "public_key", "proposer"];
    for present in ["producer_pubkey", "producer_sig"] {
        assert!(
            !legacy_banned.iter().any(|b| present.contains(b)),
            "`{present}` DOES contain a legacy banned substring, so the original \
             tripwire would have fired and this test's premise is wrong — \
             re-read why it passed on main before trusting this file"
        );
    }
}

/// #2219 V1 gate: this test binary never installs a signing protocol version,
/// so the production paths here run at the V1 default — and at V1 the producer
/// must NOT stamp a post-state-root commitment. The key entering V1 metadata
/// would move every existing block hash, which is exactly what the V2 gate
/// exists to prevent before the #2034 fresh-genesis cut.
#[tokio::test]
async fn v1_producer_emits_no_state_root_commitment() {
    use wrfcoin_shared_protocol::blockchain::block::STATE_ROOT_METADATA_KEY;
    use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;

    assert_eq!(
        SigningProtocolVersion::active(),
        SigningProtocolVersion::V1,
        "precondition: this binary must run at the V1 default"
    );

    let (_, producer) = keypair(2_219_001);
    let (block, accepted) = produce_and_verify(&producer.to_string()).await;

    assert!(
        !block.metadata.contains_key(STATE_ROOT_METADATA_KEY),
        "V1 blocks must not carry a state_root commitment — V1 hashes are frozen"
    );
    assert!(accepted, "V1 verification must be unaffected by #2219");
}
