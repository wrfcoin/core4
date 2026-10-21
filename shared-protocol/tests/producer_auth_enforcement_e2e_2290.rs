//! END-TO-END enforcement spec for block-producer authentication (#2290,
//! Option B — governance-managed dedicated set, enforce-at-genesis, stable
//! payout_address).
//!
//! Exercises the ONE shared gate `verify_block_producer_authorized` that BOTH
//! untrusted acceptance boundaries call — gossip (`pop_adapter::validate_block`)
//! and IBD (`p2p::sync::validate_block`) — with a **seeded** producer set, the
//! form a fresh-genesis chain carries. (Boundary-integration tests live in
//! `wrfcoin-p2p` (sync.rs) and `wrfcoin-consensus-engines` (pop_adapter), which
//! can construct the full validators; this file pins the shared authority logic.)
//!
//! These were RED before the gate was wired; they are GREEN now — the flip that
//! proves enforcement is live:
//!   signed seeded member ACCEPTED; unsigned REJECTED; signed non-member REJECTED
//!   solely for membership; member paying the wrong address REJECTED; and an
//!   un-seeded (legacy) chain is NOT enforced.
//!
//! Run:  cargo test -p wrfcoin-shared-protocol --test producer_auth_enforcement_e2e_2290

use rand::SeedableRng;
use rand::rngs::StdRng;

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::transaction::{Transaction, TransactionOutput};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::block_producer_auth::sign_block_producer;
use wrfcoin_shared_protocol::consensus::producer_authority::verify_block_producer_authorized;
use wrfcoin_shared_protocol::consensus::producer_set::{ProducerMember, ProducerSetChange};
use wrfcoin_shared_protocol::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// Canonical payout `Address` (0x-hex) — the coinbase form the mining route
/// accepts, NOT `pk.to_address()` (wrf form). §R2.
fn payout_of(pk: &PublicKey) -> String {
    let key: [u8; 32] = pk.as_bytes().try_into().unwrap();
    Address(key[0..20].try_into().unwrap()).to_string()
}

fn member_of(pk: &PublicKey, id: &str) -> ProducerMember {
    ProducerMember {
        producer_id: id.to_string(),
        payout_address: payout_of(pk),
        public_key: pk.as_bytes().try_into().unwrap(),
    }
}

/// The genesis-seeded producer-set history (what a fresh-genesis chain carries).
fn seeded(members: Vec<ProducerMember>) -> Vec<ProducerSetChange> {
    vec![ProducerSetChange::new(0, 0, members)]
}

/// A height-1 block whose coinbase pays `recipient`, signed by `signer` when set.
fn block_h1(recipient: &str, signer: Option<&PrivateKey>) -> Block {
    let mut coinbase = Transaction::new(vec![], vec![TransactionOutput {
        amount: 5000,
        address: recipient.to_string(),
        script: "OP_TRUE".to_string(),
        nft_data: None,
    }]);
    coinbase.set_id();
    let mut b = Block::new(3, Hash::default(), vec![coinbase], 1, 1);
    if let Some(sk) = signer {
        let att = sign_block_producer(&b, sk, ChainIdentity::active(), SigningProtocolVersion::active());
        b.producer_pubkey = att.producer_pubkey;
        b.producer_sig = att.producer_sig.to_vec();
    }
    b
}

#[test]
fn seeded_member_signed_block_is_accepted() {
    let (sk, pk) = keypair(1);
    let member = member_of(&pk, "m");
    let history = seeded(vec![member.clone()]);
    let block = block_h1(&member.payout_address, Some(&sk));
    assert_eq!(verify_block_producer_authorized(&block, &history), Ok(()));
}

#[test]
fn unsigned_block_is_rejected() {
    let (_, pk) = keypair(2);
    let member = member_of(&pk, "m");
    let history = seeded(vec![member.clone()]);
    let block = block_h1(&member.payout_address, None);
    assert!(
        verify_block_producer_authorized(&block, &history).is_err(),
        "unsigned block must be rejected under a seeded set (#2290)"
    );
}

#[test]
fn signed_non_member_is_rejected_for_membership() {
    let (_, member_pk) = keypair(3);
    let (stranger_sk, stranger_pk) = keypair(4);
    let history = seeded(vec![member_of(&member_pk, "m")]);
    // Stranger signs and pays ITS OWN valid payout → only reason to reject is
    // non-membership.
    let block = block_h1(&payout_of(&stranger_pk), Some(&stranger_sk));
    assert!(
        verify_block_producer_authorized(&block, &history).is_err(),
        "a signed non-member must be rejected solely for non-membership (#2290)"
    );
}

#[test]
fn member_paying_wrong_address_is_rejected() {
    let (sk, pk) = keypair(5);
    let (_, other_pk) = keypair(6);
    let history = seeded(vec![member_of(&pk, "m")]);
    // Authorised member signs, but coinbase pays a DIFFERENT address than its
    // stable payout_address → rejected (§R2).
    let block = block_h1(&payout_of(&other_pk), Some(&sk));
    assert!(
        verify_block_producer_authorized(&block, &history).is_err(),
        "a member must pay its own stable payout_address (#2290 §R2)"
    );
}

/// **Seeded-but-empty is NOT the same value as unseeded, and it is fail-closed.**
///
/// The activation rule is "enforce IFF seeded", implemented as
/// `producer_set_history.is_empty()`. That invites a reading in which a chain
/// that seeded a set with *no members* is indistinguishable from a chain that
/// never seeded one — which would make the fail-closed guarantee unenforceable,
/// since the dangerous state would silently take the unenforced branch.
///
/// It does not, and this test is here so nobody has to re-derive why. The two
/// states have distinct representations:
///
/// | chain state | representation | verdict |
/// |---|---|---|
/// | never configured a producer set | `[]` — an empty history | `Ok(())`, unenforced |
/// | configured a set with no members | `[ProducerSetChange { members: [] }]` — a **non-empty** history holding an empty change | `Err`, fail-closed |
///
/// The second is rejected by `validate_members` as
/// `ProducerSetError::EmptyMemberSet` before any block is judged, because
/// `verify_block_producer_authorized` only short-circuits on an empty *history*
/// and otherwise runs the history through validation.
///
/// Pinned as a test because the distinction lives in two places that a
/// well-meaning simplification would merge: the `is_empty()` short-circuit here,
/// and the emptiness check inside `validate_members`. Delete either and a chain
/// that seeds an empty set stops being enforced while still looking configured —
/// on a fresh-genesis chain that is a chain which appears authenticated and
/// enforces nothing.
#[test]
fn seeded_with_no_members_is_fail_closed_and_distinct_from_unseeded() {
    let (_, pk) = keypair(8);
    let block = block_h1(&payout_of(&pk), None);

    // Unseeded: no producer-auth chain, not enforced.
    assert_eq!(
        verify_block_producer_authorized(&block, &[]),
        Ok(()),
        "an empty history means the chain never configured a producer set"
    );

    // Seeded with an empty member list: a DIFFERENT value, and fail-closed.
    let seeded_empty = vec![ProducerSetChange::new(0, 0, vec![])];
    let verdict = verify_block_producer_authorized(&block, &seeded_empty);
    assert!(
        verdict.is_err(),
        "a seeded set with no members must be FAIL-CLOSED, not silently \
         unenforced — got {verdict:?}"
    );
    let reason = verdict.unwrap_err();
    assert!(
        reason.contains("EmptyMemberSet"),
        "the refusal must come from the empty-member-set validation, so the \
         guarantee is attributable rather than incidental — got: {reason}"
    );

    // And the two really are different inputs, not the same value twice.
    assert!(
        !seeded_empty.is_empty(),
        "precondition: a seeded-but-empty history is a NON-empty slice, which is \
         exactly why it does not take the unenforced short-circuit"
    );
}

#[test]
fn unseeded_legacy_chain_is_not_enforced() {
    // No producer set configured → not a producer-auth chain → an unsigned block
    // passes. This is the activation semantics (enforce IFF seeded); the fresh
    // genesis seeds the set so it is enforced there.
    let (_, pk) = keypair(7);
    let block = block_h1(&payout_of(&pk), None);
    assert_eq!(verify_block_producer_authorized(&block, &[]), Ok(()));
}


// ---------------------------------------------------------------------------
// #2290 boot backstop: a persisted chain containing an unauthorized block must
// refuse to start, whatever route put the block there.
// ---------------------------------------------------------------------------

use wrfcoin_shared_protocol::consensus::producer_authority::verify_persisted_blocks_authorized;

#[test]
fn boot_check_refuses_a_persisted_chain_containing_an_unauthorized_block() {
    let (sk, pk) = keypair(1);
    let history = seeded(vec![member_of(&pk, "n5-a")]);
    let payout = payout_of(&pk);

    // Height 1 authorized; height 2 signed by a key NOT in the set — the shape
    // a node ends up with after mining locally without the production gate.
    let (rogue_sk, _) = keypair(999);
    let good = block_h1(&payout, Some(&sk));
    let bad = block_h1(&payout, Some(&rogue_sk));

    let blocks = vec![(1u64, good), (2u64, bad)];
    let refs: Vec<(&u64, &Block)> = blocks.iter().map(|(h, b)| (h, b)).collect();

    let err = verify_persisted_blocks_authorized(refs, &history)
        .expect_err("a persisted unauthorized block must refuse the boot");
    assert_eq!(err.0, 2, "must point at the offending height, got {err:?}");
}

#[test]
fn boot_check_passes_when_every_persisted_block_is_authorized() {
    let (sk, pk) = keypair(1);
    let history = seeded(vec![member_of(&pk, "n5-a")]);
    let payout = payout_of(&pk);

    let blocks = vec![
        (1u64, block_h1(&payout, Some(&sk))),
        (2u64, block_h1(&payout, Some(&sk))),
    ];
    let refs: Vec<(&u64, &Block)> = blocks.iter().map(|(h, b)| (h, b)).collect();

    assert_eq!(
        verify_persisted_blocks_authorized(refs, &history),
        Ok(2),
        "an all-authorized chain must boot, and report how many it checked"
    );
}

/// An unconfigured chain must still boot — the backstop must not brick legacy
/// or dev chains that never seeded a producer set.
#[test]
fn boot_check_is_unenforced_when_no_producer_set_was_ever_configured() {
    let (rogue_sk, rogue_pk) = keypair(4242);
    let blocks = vec![(1u64, block_h1(&payout_of(&rogue_pk), Some(&rogue_sk)))];
    let refs: Vec<(&u64, &Block)> = blocks.iter().map(|(h, b)| (h, b)).collect();

    assert_eq!(
        verify_persisted_blocks_authorized(refs, &[]),
        Ok(0),
        "empty history = never configured = unenforced, same as the other gates"
    );
}
