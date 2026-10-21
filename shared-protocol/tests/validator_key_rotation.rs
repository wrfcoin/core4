//! Validator key rotation on a multi-validator fixture (#2130).
//!
//! # What this fixture is, and what it is not
//!
//! It is a deterministic multi-validator set that produces and verifies a chain
//! of blocks across a rotation boundary, asserting that **no height goes
//! unproduced or unverified**. Every validator signs, every block is verified
//! by every other validator, and the rotating validator's key changes mid-run.
//!
//! It is **not** a network fixture — no sockets, no processes. That is
//! deliberate for a test that must run in the default path: the property under
//! test ("does the rotation leave a height with no acceptable key") is a
//! property of the key-acceptance rules, not of the transport. A socket-based
//! fixture would test tokio as much as rotation, and would be the first test
//! marked `#[ignore]` when it flaked.
//!
//! The transport-level companion — rotating a key on a running multi-node
//! testnet — is an operator procedure, written up in
//! `docs/security/KEY_ROTATION_RUNBOOK.md`, because it needs real nodes.
//!
//! # Determinism
//!
//! Seeded keys, no wall-clock reads, no unordered iteration. A rotation test
//! that fails one run in fifty is a test that gets deleted.

use std::collections::BTreeMap;

use wrfcoin_shared_protocol::crypto::rotation::{
    MIN_OVERLAP_BLOCKS, RotationPlan, rotation_pop_message,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};

use rand::SeedableRng;
use rand::rngs::StdRng;

const VALIDATOR_COUNT: usize = 4;
/// The validator that rotates its key mid-run.
const ROTATING: usize = 2;
const EFFECTIVE_HEIGHT: u64 = 100;
const CHAIN_LENGTH: u64 = 130;

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// A validator that may hold two keys across a rotation.
struct Validator {
    index: usize,
    old: (PrivateKey, PublicKey),
    /// `Some` only for the rotating validator.
    new: Option<(PrivateKey, PublicKey)>,
    plan: Option<RotationPlan>,
}

impl Validator {
    /// The key this validator signs with at `height`.
    ///
    /// Switches at `effective_height`. Signing early would be caught by the
    /// verifier; signing late would miss blocks.
    fn signing_key_at(&self, height: u64) -> &(PrivateKey, PublicKey) {
        match (&self.new, &self.plan) {
            (Some(new), Some(plan)) if height >= plan.effective_height => new,
            _ => &self.old,
        }
    }

    fn sign_block(&self, height: u64, payload: &[u8]) -> (PublicKey, Signature) {
        let (sk, pk) = self.signing_key_at(height);
        (pk.clone(), sk.sign(payload))
    }

    /// Verify another validator's block signature under the rotation rules.
    fn verify_block(
        &self,
        author: &Validator,
        height: u64,
        payload: &[u8],
        key: &PublicKey,
        sig: &Signature,
    ) -> bool {
        // Is this key authoritative for the author at this height?
        let authorised = match &author.plan {
            Some(plan) => plan.accepts_key_at(key, height),
            None => *key == author.old.1,
        };
        authorised && key.verify(payload, sig)
    }
}

fn block_payload(height: u64, author: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(24);
    buf.extend_from_slice(b"wrfcoin.block.v1");
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(&(author as u64).to_le_bytes());
    buf
}

/// Build the validator set with validator `ROTATING` carrying a valid plan.
fn validator_set() -> Vec<Validator> {
    (0..VALIDATOR_COUNT)
        .map(|index| {
            let old = keypair(1_000 + index as u64);
            if index == ROTATING {
                let new = keypair(2_000 + index as u64);
                let message = rotation_pop_message(&old.1, &new.1, EFFECTIVE_HEIGHT);
                let plan = RotationPlan {
                    old: old.1.clone(),
                    new: new.1.clone(),
                    effective_height: EFFECTIVE_HEIGHT,
                    overlap_blocks: MIN_OVERLAP_BLOCKS,
                    proof: Some(new.0.sign(&message)),
                };
                Validator {
                    index,
                    old,
                    new: Some(new),
                    plan: Some(plan),
                }
            } else {
                Validator {
                    index,
                    old,
                    new: None,
                    plan: None,
                }
            }
        })
        .collect()
}

#[test]
fn plan_is_valid_before_the_run() {
    let set = validator_set();
    let plan = set[ROTATING].plan.as_ref().unwrap();
    assert_eq!(plan.validate(0), Ok(()));
}

#[test]
fn rotation_completes_with_no_missed_blocks() {
    // The headline acceptance criterion. Every height must be produced by every
    // validator and accepted by every other validator, across the boundary.
    let set = validator_set();
    let mut accepted_per_height: BTreeMap<u64, usize> = BTreeMap::new();

    for height in 1..=CHAIN_LENGTH {
        for author in &set {
            let payload = block_payload(height, author.index);
            let (key, sig) = author.sign_block(height, &payload);

            for verifier in &set {
                assert!(
                    verifier.verify_block(author, height, &payload, &key, &sig),
                    "validator {} rejected validator {}'s block at height {height} — \
                     a missed block across the rotation boundary",
                    verifier.index,
                    author.index
                );
            }
            *accepted_per_height.entry(height).or_default() += 1;
        }
    }

    assert_eq!(accepted_per_height.len(), CHAIN_LENGTH as usize);
    for (height, count) in &accepted_per_height {
        assert_eq!(
            *count, VALIDATOR_COUNT,
            "height {height} was produced by {count} validators, expected {VALIDATOR_COUNT}"
        );
    }
}

#[test]
fn the_rotating_validator_actually_changed_keys_mid_run() {
    // Guards against the previous test passing because nothing rotated.
    let set = validator_set();
    let v = &set[ROTATING];
    let before = v.signing_key_at(EFFECTIVE_HEIGHT - 1).1.clone();
    let after = v.signing_key_at(EFFECTIVE_HEIGHT).1.clone();
    assert_ne!(before, after, "the rotating validator never switched keys");
    assert_eq!(before, v.old.1);
    assert_eq!(after, v.new.as_ref().unwrap().1);
}

#[test]
fn old_key_is_rejected_after_effective_height_on_the_fixture() {
    let set = validator_set();
    let author = &set[ROTATING];
    let verifier = &set[0];

    let height = EFFECTIVE_HEIGHT + 1;
    let payload = block_payload(height, author.index);
    // Author deliberately signs with the retired key.
    let (old_sk, old_pk) = &author.old;
    let sig = old_sk.sign(&payload);

    assert!(
        !verifier.verify_block(author, height, &payload, old_pk, &sig),
        "the retired key must be rejected after the effective height"
    );
}

#[test]
fn new_key_is_rejected_before_its_overlap_window_on_the_fixture() {
    let set = validator_set();
    let author = &set[ROTATING];
    let verifier = &set[0];
    let plan = author.plan.as_ref().unwrap();

    let height = plan.overlap_start() - 1;
    let payload = block_payload(height, author.index);
    let (new_sk, new_pk) = author.new.as_ref().unwrap();
    let sig = new_sk.sign(&payload);

    assert!(
        !verifier.verify_block(author, height, &payload, new_pk, &sig),
        "the new key must not be usable before its overlap window opens"
    );
}

#[test]
fn both_keys_verify_inside_the_overlap_window_on_the_fixture() {
    // The specific mechanism that prevents missed blocks: a block signed just
    // before the boundary is still accepted inside the window.
    let set = validator_set();
    let author = &set[ROTATING];
    let verifier = &set[0];
    let plan = author.plan.as_ref().unwrap();

    let height = plan.overlap_start() + 1;
    let payload = block_payload(height, author.index);

    let (old_sk, old_pk) = &author.old;
    let (new_sk, new_pk) = author.new.as_ref().unwrap();

    assert!(verifier.verify_block(author, height, &payload, old_pk, &old_sk.sign(&payload)));
    assert!(verifier.verify_block(author, height, &payload, new_pk, &new_sk.sign(&payload)));
}

#[test]
fn a_foreign_key_is_never_accepted_for_a_validator() {
    // Rotation must not widen the accepted set to "any key with a valid
    // signature" — the signature check alone would pass here.
    let set = validator_set();
    let author = &set[ROTATING];
    let verifier = &set[0];
    let (foreign_sk, foreign_pk) = keypair(9_999);

    for height in [1, EFFECTIVE_HEIGHT - 1, EFFECTIVE_HEIGHT, CHAIN_LENGTH] {
        let payload = block_payload(height, author.index);
        let sig = foreign_sk.sign(&payload);
        assert!(
            !verifier.verify_block(author, height, &payload, &foreign_pk, &sig),
            "a foreign key was accepted for the rotating validator at height {height}"
        );
    }
}

#[test]
fn non_rotating_validators_are_unaffected() {
    // A rotation by one validator must not change acceptance for the others.
    let set = validator_set();
    let verifier = &set[0];
    for author in set.iter().filter(|v| v.index != ROTATING) {
        for height in [1, EFFECTIVE_HEIGHT, CHAIN_LENGTH] {
            let payload = block_payload(height, author.index);
            let (key, sig) = author.sign_block(height, &payload);
            assert!(verifier.verify_block(author, height, &payload, &key, &sig));
        }
    }
}

// ---------------------------------------------------------------------------
// Independently-configured peers (review of PR #2241)
// ---------------------------------------------------------------------------
//
// The reviewer's second blocker was that `validator_set()` hands every
// validator the *same* `RotationPlan`, so the fixture cannot catch the
// operational failure it most needs to: **only one node activates the new
// policy**. That is the same class Elsa found in #2232, and the reason #2076
// halted the chain for 28 hours.
//
// These tests give each validator its own independently-configured view — a
// peer that never received the plan, and a peer that received a *different*
// one — and assert the divergence is detected rather than silently accepted.

/// Build a verifier whose knowledge of the rotation is configured per-node,
/// rather than shared from one object.
fn verifier_with_plan(index: usize, plan: Option<RotationPlan>) -> Validator {
    Validator {
        index,
        old: keypair(1_000 + index as u64),
        new: None,
        plan,
    }
}

#[test]
fn a_peer_that_never_received_the_plan_rejects_the_rotated_key() {
    // The one-node-diverges case. A verifier still on the pre-rotation config
    // must NOT accept blocks signed with the new key — and the mismatch must be
    // visible, not silently tolerated.
    let set = validator_set();
    let author = &set[ROTATING];

    // This peer's view of the rotating validator carries no plan at all.
    let stale_peer = verifier_with_plan(0, None);
    let mut author_as_stale_peer_sees_it = verifier_with_plan(ROTATING, None);
    author_as_stale_peer_sees_it.old = keypair(1_000 + ROTATING as u64);

    let height = EFFECTIVE_HEIGHT + 1;
    let payload = block_payload(height, author.index);
    let (new_sk, new_pk) = author.new.as_ref().unwrap();
    let sig = new_sk.sign(&payload);

    assert!(
        !stale_peer.verify_block(
            &author_as_stale_peer_sees_it,
            height,
            &payload,
            new_pk,
            &sig
        ),
        "a peer that never received the rotation plan must reject the new key — \
         accepting it would mean the plan was never needed, and rejecting it \
         silently is how one node diverges from the fleet"
    );
}

#[test]
fn a_peer_holding_a_different_effective_height_diverges_detectably() {
    // Plan distribution can go wrong by *content*, not just absence: two peers
    // with different `effective_height` values disagree about a real block.
    let set = validator_set();
    let author = &set[ROTATING];
    let (new_sk, new_pk) = author.new.as_ref().unwrap();

    let agreed = author.plan.clone().unwrap();
    let mut skewed = agreed.clone();
    skewed.effective_height = EFFECTIVE_HEIGHT + 50;

    let peer_on_time = verifier_with_plan(0, Some(agreed));
    let peer_skewed = verifier_with_plan(1, Some(skewed));

    let mut author_view = verifier_with_plan(ROTATING, author.plan.clone());
    author_view.old = keypair(1_000 + ROTATING as u64);

    // A block just after the agreed boundary, signed with the new key.
    let height = EFFECTIVE_HEIGHT + 1;
    let payload = block_payload(height, author.index);
    let sig = new_sk.sign(&payload);

    let accepted_on_time = peer_on_time.verify_block(&author_view, height, &payload, new_pk, &sig);

    let mut author_view_skewed = verifier_with_plan(ROTATING, peer_skewed.plan.clone());
    author_view_skewed.old = keypair(1_000 + ROTATING as u64);
    let accepted_skewed =
        peer_skewed.verify_block(&author_view_skewed, height, &payload, new_pk, &sig);

    // This is the failure class the reviewer said the shared-plan fixture could
    // not reach, and it is sharper than expected: the skewed peer's overlap
    // window has not opened yet at this height, so it REJECTS a block the
    // correctly-configured peer ACCEPTS. One node, one wrong config value, and
    // the fleet disagrees about a real block — #2076 in miniature.
    assert!(
        accepted_on_time,
        "the correctly-configured peer must accept"
    );
    assert!(
        !accepted_skewed,
        "a peer whose effective_height is skewed forward must reject a block the \
         on-time peer accepts — if both accepted, plan distribution would not \
         matter and this whole mechanism would be theatre"
    );

    // The detectable divergence: past the skewed peer's own boundary, the OLD
    // key is still accepted by it while the on-time peer has already retired it.
    let late = EFFECTIVE_HEIGHT + 10;
    let late_payload = block_payload(late, author.index);
    let old_sig = author.old.0.sign(&late_payload);
    let on_time_accepts_old =
        peer_on_time.verify_block(&author_view, late, &late_payload, &author.old.1, &old_sig);
    let skewed_accepts_old = peer_skewed.verify_block(
        &author_view_skewed,
        late,
        &late_payload,
        &author.old.1,
        &old_sig,
    );
    assert!(
        !on_time_accepts_old && skewed_accepts_old,
        "peers with different effective heights must disagree about the old key \
         after the boundary — that observable disagreement is what an operator \
         must be able to detect before it becomes a silent halt"
    );
}

#[test]
fn fixture_is_deterministic_across_runs() {
    // Same seeds must give the same keys, or a failure is unreproducible.
    let a = validator_set();
    let b = validator_set();
    for (x, y) in a.iter().zip(b.iter()) {
        assert_eq!(x.old.1, y.old.1);
        assert_eq!(
            x.new.as_ref().map(|n| n.1.clone()),
            y.new.as_ref().map(|n| n.1.clone())
        );
    }
}

// ---------------------------------------------------------------------------
// Production path (#2130 round-2 review)
//
// Everything above this line exercises the rotation *model* against a local
// fixture. The review's blocker was precisely that: the model was authoritative
// only inside the fixture, because production constructed a raw signer and no
// production code ever built the rotation wrapper or consulted the plan.
//
// These tests go through `load_block_signer` — the same function `launch_api`
// calls at startup — and assert enforcement at production heights via the
// `BlockSigner` trait method that production signing sites now call.
// ---------------------------------------------------------------------------

use std::sync::{Mutex, MutexGuard, OnceLock};
use wrfcoin_shared_protocol::crypto::rotation::FailClosedRemoteSigner;
use wrfcoin_shared_protocol::crypto::signer::{BlockSigner, env_vars, load_block_signer};

/// Rotation config lives in process-global env, so these tests serialise.
fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

const SIGNER_KEY_VAR: &str = "WRFCOIN_TEST_2130_PRODUCTION_SIGNER_KEY";

/// Clear every variable these tests set, so one test cannot leak into another.
fn clear_rotation_env() {
    for var in [
        env_vars::ROTATION_OLD_PUBKEY,
        env_vars::ROTATION_NEW_PUBKEY,
        env_vars::ROTATION_EFFECTIVE_HEIGHT,
        env_vars::ROTATION_OVERLAP_BLOCKS,
        env_vars::ROTATION_POP,
        "WRFCOIN_ROTATION_SCOPE_ACK", // removed in #2290; cleared so leftovers cannot leak
        SIGNER_KEY_VAR,
    ] {
        unsafe { std::env::remove_var(var) };
    }
}

/// Configure the environment so `load_block_signer` resolves a signer holding
/// `old_sk` and rotating to `new_pk` at `effective_height`.
fn set_rotation_env(
    old_sk: &PrivateKey,
    old_pk: &PublicKey,
    new_sk: &PrivateKey,
    new_pk: &PublicKey,
    effective_height: u64,
) {
    let pop = new_sk.sign(&rotation_pop_message(old_pk, new_pk, effective_height));
    unsafe {
        std::env::set_var(SIGNER_KEY_VAR, old_sk.to_hex());
        std::env::set_var(env_vars::ROTATION_OLD_PUBKEY, old_pk.to_hex());
        std::env::set_var(env_vars::ROTATION_NEW_PUBKEY, new_pk.to_hex());
        std::env::set_var(
            env_vars::ROTATION_EFFECTIVE_HEIGHT,
            effective_height.to_string(),
        );
        std::env::set_var(
            env_vars::ROTATION_OVERLAP_BLOCKS,
            MIN_OVERLAP_BLOCKS.to_string(),
        );
        std::env::set_var(env_vars::ROTATION_POP, pop.to_hex());
        // #2290: the round-4 scope acknowledgement is GONE. It made the operator
        // affirm that rotation covered payload signing only and NOT block
        // production; blocks now carry a producer signature that is signed at
        // their height, so that statement is false and the gate was removed.
        // Nothing to set here any more.
    }
}

/// **The test the review asked for.** Rotation enforced at production height,
/// through the production constructor: the old key signs inside its overlap
/// window and is refused at and after the boundary.
#[test]
fn production_signer_enforces_the_rotation_boundary_at_height() {
    let _guard = env_lock();
    clear_rotation_env();

    let (old_sk, old_pk) = keypair(900);
    let (new_sk, new_pk) = keypair(901);
    const EFFECTIVE: u64 = 1_000_000;
    set_rotation_env(&old_sk, &old_pk, &new_sk, &new_pk, EFFECTIVE);

    let resolved = load_block_signer(SIGNER_KEY_VAR)
        .expect("rotation config is valid")
        .expect("a signer is configured");

    // Production gets the wrapper, not a raw signer — the blocker was that it
    // did not. Asserted structurally via Debug, since the trait object hides
    // the concrete type.
    let rendered = format!("{:?}", resolved.signer);
    assert!(
        rendered.contains("FailClosedRemoteSigner"),
        "load_block_signer must return the rotation wrapper, got: {rendered}"
    );
    assert!(
        rendered.contains("rotating: true"),
        "the configured rotation plan must be attached, got: {rendered}"
    );

    let payload = b"block-header-bytes";

    // Inside the overlap window the old key is still authoritative.
    let before = EFFECTIVE - 1;
    assert!(
        before >= EFFECTIVE - MIN_OVERLAP_BLOCKS,
        "sanity: {before} must be inside the overlap window"
    );
    resolved
        .signer
        .sign_at_height(payload, before)
        .expect("the old key must still sign inside its overlap window");

    // At the boundary the old key is revoked.
    let at_boundary = resolved.signer.sign_at_height(payload, EFFECTIVE);
    assert!(
        at_boundary.is_err(),
        "the old key must be refused AT the effective height — this is revocation \
         being authoritative on the signing path, not merely in the model"
    );

    // And it stays revoked afterwards.
    assert!(
        resolved
            .signer
            .sign_at_height(payload, EFFECTIVE + 5_000)
            .is_err(),
        "the old key must stay refused after the boundary"
    );

    clear_rotation_env();
}

/// **The other half the review asked for: the NEW key actually takes over.**
///
/// The old-key test above proves revocation; this proves *adoption* through the
/// production constructor, not the in-memory `signing_key_at` fixture. It models
/// the operator step "switch the signer here" (runbook §6): the configured
/// signer is now the new private key, everything else about the plan unchanged.
/// `load_block_signer` reloads, and we prove the swap took effect end to end —
/// `public_key()` is the new key, it signs at/after the boundary, the signature
/// verifies under the NEW key and NOT the old one, and it is refused before the
/// overlap window opens. A silent no-op reload (still the old key, or a signer
/// with no plan) fails every one of these.
#[test]
fn production_new_signer_becomes_authoritative_after_the_operator_swaps_it() {
    let _guard = env_lock();
    clear_rotation_env();

    let (old_sk, old_pk) = keypair(905);
    let (new_sk, new_pk) = keypair(906);
    const EFFECTIVE: u64 = 1_500_000;
    // Same plan (POP is still signed by the new key over old->new@EFFECTIVE)…
    set_rotation_env(&old_sk, &old_pk, &new_sk, &new_pk, EFFECTIVE);
    // …but the operator has switched the CONFIGURED signer to the new key.
    unsafe { std::env::set_var(SIGNER_KEY_VAR, new_sk.to_hex()) };

    let resolved = load_block_signer(SIGNER_KEY_VAR)
        .expect("rotation config is valid")
        .expect("a signer is configured");

    // The swap is visible through the production wrapper, not asserted.
    assert_eq!(
        resolved.signer.public_key().to_hex(),
        new_pk.to_hex(),
        "after the operator switches the configured signer, load_block_signer must \
         load the NEW key — a no-op reload would still report the old key"
    );
    assert_ne!(
        new_pk.to_hex(),
        old_pk.to_hex(),
        "sanity: the two keys must differ or this test proves nothing"
    );

    let payload = b"block-header-bytes";

    // At and after the boundary the new key is authoritative and its signature
    // verifies under the new key — and NOT under the old one.
    let sig = resolved
        .signer
        .sign_at_height(payload, EFFECTIVE)
        .expect("the new key must sign at its effective height");
    assert!(
        new_pk.verify(payload, &sig),
        "the signature at the boundary must verify under the NEW key"
    );
    assert!(
        !old_pk.verify(payload, &sig),
        "the boundary signature must NOT verify under the old key — this is the new \
         key genuinely being used, not the old one relabelled"
    );

    // Inside the overlap window the new key is also accepted (both keys valid).
    let inside_overlap = EFFECTIVE - 1;
    assert!(
        inside_overlap >= EFFECTIVE - MIN_OVERLAP_BLOCKS,
        "sanity: {inside_overlap} must be inside the overlap window"
    );
    resolved
        .signer
        .sign_at_height(payload, inside_overlap)
        .expect("the new key must sign inside the overlap window");

    // Before the overlap opens the new key is NOT yet authoritative: signing is
    // refused. Rotation adopts the new key on a schedule, not the moment the
    // operator swaps the signer.
    let before_overlap = EFFECTIVE - MIN_OVERLAP_BLOCKS - 1;
    assert!(
        resolved
            .signer
            .sign_at_height(payload, before_overlap)
            .is_err(),
        "the new key must be refused before its overlap window opens (height {before_overlap})"
    );

    clear_rotation_env();
}

/// The structural half: a caller that forgets to pass a height cannot silently
/// bypass rotation, because the heightless path refuses while a plan is active.
///
/// Before this change `BlockSigner::sign` delegated straight to the inner
/// signer and ignored the plan, so every production call site bypassed rotation
/// while the model's own tests passed.
#[test]
fn production_heightless_signing_is_refused_while_rotating() {
    let _guard = env_lock();
    clear_rotation_env();

    let (old_sk, old_pk) = keypair(902);
    let (new_sk, new_pk) = keypair(903);
    set_rotation_env(&old_sk, &old_pk, &new_sk, &new_pk, 2_000_000);

    let resolved = load_block_signer(SIGNER_KEY_VAR).unwrap().unwrap();

    let err = resolved
        .signer
        .sign(b"block-header-bytes")
        .expect_err("heightless signing must refuse during a rotation");
    let msg = err.to_string();
    assert!(
        msg.contains("sign_at_height"),
        "the refusal must tell the caller what to do instead, got: {msg}"
    );

    clear_rotation_env();
}

/// Without a rotation configured the wrapper is transparent — no behaviour
/// change for the overwhelmingly common case.
#[test]
fn production_signing_is_unchanged_when_not_rotating() {
    let _guard = env_lock();
    clear_rotation_env();

    let (sk, _pk) = keypair(904);
    unsafe { std::env::set_var(SIGNER_KEY_VAR, sk.to_hex()) };

    let resolved = load_block_signer(SIGNER_KEY_VAR).unwrap().unwrap();
    let rendered = format!("{:?}", resolved.signer);
    assert!(rendered.contains("rotating: false"), "got: {rendered}");

    let payload = b"block-header-bytes";
    let heightless = resolved.signer.sign(payload).expect("no plan, no refusal");
    let at_height = resolved
        .signer
        .sign_at_height(payload, 12_345)
        .expect("no plan, no refusal");
    assert_eq!(
        heightless.to_hex(),
        at_height.to_hex(),
        "with no rotation the two paths must be identical — ed25519 is deterministic"
    );

    clear_rotation_env();
}

/// A half-configured rotation is a refusal to start, never a silent downgrade
/// to unrotated signing.
#[test]
fn a_partially_configured_rotation_refuses_to_start() {
    let _guard = env_lock();
    clear_rotation_env();

    let (sk, _pk) = keypair(905);
    let (_, new_pk) = keypair(906);
    unsafe {
        std::env::set_var(SIGNER_KEY_VAR, sk.to_hex());
        // New key named, everything else missing.
        std::env::set_var(env_vars::ROTATION_NEW_PUBKEY, new_pk.to_hex());
    }

    let err = load_block_signer(SIGNER_KEY_VAR)
        .expect_err("a partial rotation config must refuse, not fall back to unrotated");
    assert!(
        err.to_string().contains(env_vars::ROTATION_OLD_PUBKEY),
        "the error must name the missing variable, got: {err}"
    );

    clear_rotation_env();
}

/// An unproven new key cannot be rotated to, even through the env path — the
/// env constructor must not route around `RotationPlan::validate`.
#[test]
fn a_rotation_with_a_forged_proof_refuses_to_start() {
    let _guard = env_lock();
    clear_rotation_env();

    let (old_sk, old_pk) = keypair(907);
    let (_new_sk, new_pk) = keypair(908);
    let (impostor_sk, _) = keypair(909);
    const EFFECTIVE: u64 = 3_000_000;

    // Proof signed by a key that is NOT the new key.
    let forged = impostor_sk.sign(&rotation_pop_message(&old_pk, &new_pk, EFFECTIVE));
    unsafe {
        std::env::set_var(SIGNER_KEY_VAR, old_sk.to_hex());
        std::env::set_var(env_vars::ROTATION_OLD_PUBKEY, old_pk.to_hex());
        std::env::set_var(env_vars::ROTATION_NEW_PUBKEY, new_pk.to_hex());
        std::env::set_var(env_vars::ROTATION_EFFECTIVE_HEIGHT, EFFECTIVE.to_string());
        std::env::set_var(
            env_vars::ROTATION_OVERLAP_BLOCKS,
            MIN_OVERLAP_BLOCKS.to_string(),
        );
        std::env::set_var(env_vars::ROTATION_POP, forged.to_hex());
    }

    let err = load_block_signer(SIGNER_KEY_VAR)
        .expect_err("a forged proof of possession must refuse to start");
    assert!(
        format!("{err}").contains("proof of possession"),
        "got: {err}"
    );

    clear_rotation_env();
}

/// Keeps `FailClosedRemoteSigner` referenced from this file so the import is
/// meaningful if the Debug-string assertions above are ever relaxed.
#[test]
fn the_wrapper_type_is_the_one_production_uses() {
    let (sk, _) = keypair(910);
    let raw: std::sync::Arc<dyn BlockSigner> = std::sync::Arc::new(
        wrfcoin_shared_protocol::crypto::signer::EnvKeySigner::from_private_key(sk, "inline"),
    );
    let wrapper = FailClosedRemoteSigner::new(raw);
    assert!(!wrapper.is_rotating());
}
