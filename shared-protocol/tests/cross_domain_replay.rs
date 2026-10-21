//! Cross-domain replay suite (#2125).
//!
//! # What this asserts
//!
//! A signature produced in any one context is invalid in **every** other
//! context. A signature generated for the testnet, for another WRF network, or
//! for a governance message must never be interpretable as a valid transaction.
//!
//! # It drives production, not a model of production
//!
//! Review of PR #2235 found the flaw exactly: the first version built its own
//! `ObjectKind`, its own tag table, its own `preimage`, and signed with raw
//! `PrivateKey::sign`. Every assertion passed and **the suite would still have
//! passed with production domain separation removed**, because it tested a
//! faithful reimplementation rather than the code it claimed to protect.
//!
//! So nothing here defines a tag or builds a preimage. The matrix reads
//! [`SIGNED_OBJECT_REGISTRY`], frames bytes with the production
//! [`domain_preimage`], and the typed tests sign and verify through
//! [`sign_domain`] / [`verify_domain`]. The single concrete edit this suite
//! exists to catch — deleting the tag from `domain_preimage` — is spelled out in
//! [`removing_domain_separation_from_production_breaks_this_suite`], which
//! reproduces it and asserts the collapse.
//!
//! # Why the matrix is registry-driven, not written out
//!
//! Hand-written pairs test the pairs someone thought of. The dangerous pair is
//! the one nobody thought of — #2065 was an attestation envelope reinterpretable
//! as a transaction.
//!
//! The previous `matrix_covers_every_object_kind_and_context` asserted that its
//! own hard-coded slice had 7 entries, which is self-certifying: it could not
//! notice the registry had grown to 18. Coverage is now derived from
//! [`ALL_SIGNED_OBJECT_KINDS`], so **registering a new signed type expands this
//! matrix automatically**, and adding one with a `DomainSeparated` impl fails
//! [`every_implemented_kind_is_tag_checked`] until someone covers it.
//!
//! # Why every object shares one body
//!
//! Every row of the matrix is signed over byte-identical content. If the bodies
//! differed, the suite would pass even with domain separation completely
//! removed — the signatures would differ because the *content* differed. Forcing
//! the content equal makes the domain tag and the chain identity the only things
//! that can distinguish them.
//!
//! **A failure in this file is a security finding and must be filed. It must
//! never be adjusted away by relaxing the assertion.**

use std::collections::BTreeSet;
use std::time::Instant;

use wrfcoin_shared_protocol::blockchain::transaction::GovernanceVotePayload;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CalibrationAttestationPayload, ComputeReceipt, OracleAttestationPayload, Transaction,
};
use wrfcoin_shared_protocol::crypto::domain_registry::{
    ALL_SIGNED_OBJECT_KINDS, SIGNED_OBJECT_REGISTRY, SignedObjectKind, entry, tag_matches_registry,
};
use wrfcoin_shared_protocol::crypto::domain_signing::{
    DomainSeparated, domain_preimage, push_length_prefixed, sign_domain, verify_domain,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};

// ---------------------------------------------------------------------------
// Contexts
// ---------------------------------------------------------------------------

/// A network a signature can belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Context {
    Testnet,
    Mainnet,
    /// A third WRF network. Present because "testnet vs mainnet" alone would
    /// pass for an implementation that special-cases those two strings.
    OtherWrfNetwork,
}

impl Context {
    fn chain_id(self) -> &'static str {
        match self {
            Self::Testnet => "wrfcoin-testnet",
            Self::Mainnet => "wrfcoin-main",
            Self::OtherWrfNetwork => "wrfcoin-shadow",
        }
    }

    fn network_id(self) -> &'static str {
        match self {
            Self::Testnet => "testnet",
            Self::Mainnet => "mainnet",
            Self::OtherWrfNetwork => "shadow",
        }
    }
}

const CONTEXTS: &[Context] = &[Context::Testnet, Context::Mainnet, Context::OtherWrfNetwork];

// ---------------------------------------------------------------------------
// Preimages — production framing, production tags
// ---------------------------------------------------------------------------

/// Body shared by every object in the matrix.
///
/// Identical on purpose — see the module docs. If this differed per kind, the
/// suite would pass with domain separation removed entirely.
const SHARED_BODY: &[u8] = b"amount=1000;to=wrf00000000000000000000000000000000000000;nonce=7";

/// Chain identity plus the shared body, framed with the production
/// length-prefixer so the context cannot be re-split out of the bytes.
fn body_for(context: Context) -> Vec<u8> {
    let mut buf = Vec::new();
    push_length_prefixed(&mut buf, context.chain_id().as_bytes());
    push_length_prefixed(&mut buf, context.network_id().as_bytes());
    push_length_prefixed(&mut buf, SHARED_BODY);
    buf
}

/// The bytes a signature commits to, built by **production**
/// [`domain_preimage`] over the tag **the production registry** carries for
/// this kind.
///
/// Nothing in this file chooses a tag. If `domain_preimage` stops prefixing the
/// tag, every kind here collapses to identical bytes and the matrix fails.
fn preimage(context: Context, kind: SignedObjectKind) -> Vec<u8> {
    let tag = entry(kind)
        .unwrap_or_else(|| panic!("{kind:?} is in ALL_SIGNED_OBJECT_KINDS but has no registry row"))
        .tag;
    domain_preimage(tag, &body_for(context))
}

fn signing_key() -> (PrivateKey, PublicKey) {
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    // Seeded: an unseeded key would make a failure unreproducible.
    let mut rng = StdRng::seed_from_u64(2125);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

fn sign(sk: &PrivateKey, context: Context, kind: SignedObjectKind) -> Signature {
    sk.sign(&preimage(context, kind))
}

fn verifies(pk: &PublicKey, context: Context, kind: SignedObjectKind, sig: &Signature) -> bool {
    pk.verify(&preimage(context, kind), sig)
}

/// Every (context, registered-kind) pair. Derived from the registry, so it grows
/// when the registry does.
fn all_pairs() -> Vec<(Context, SignedObjectKind)> {
    let mut pairs = Vec::with_capacity(CONTEXTS.len() * ALL_SIGNED_OBJECT_KINDS.len());
    for &ctx in CONTEXTS {
        for &kind in ALL_SIGNED_OBJECT_KINDS {
            pairs.push((ctx, kind));
        }
    }
    pairs
}

// ---------------------------------------------------------------------------
// The matrix
// ---------------------------------------------------------------------------

#[test]
fn full_replay_matrix_rejects_every_cross_context_verification() {
    let (sk, pk) = signing_key();
    let pairs = all_pairs();

    let mut checked = 0usize;
    let mut cross_context = 0usize;

    for &(ctx_a, kind_a) in &pairs {
        let sig = sign(&sk, ctx_a, kind_a);

        // Sanity: the signature must verify in its own context, or every
        // negative assertion below is vacuous.
        assert!(
            verifies(&pk, ctx_a, kind_a, &sig),
            "{kind_a:?} on {ctx_a:?} must verify in its own context"
        );

        for &(ctx_b, kind_b) in &pairs {
            checked += 1;
            if ctx_a == ctx_b && kind_a == kind_b {
                continue;
            }
            cross_context += 1;
            assert!(
                !verifies(&pk, ctx_b, kind_b, &sig),
                "REPLAY: a signature over {kind_a:?} on {ctx_a:?} verified as \
                 {kind_b:?} on {ctx_b:?}. This is a security finding — file it, \
                 do not relax this assertion."
            );
        }
    }

    let expected = pairs.len();
    assert_eq!(checked, expected * expected);
    assert_eq!(cross_context, expected * expected - expected);
}

#[test]
fn every_context_object_pair_has_a_distinct_preimage() {
    // Stronger and more diagnosable than the signature matrix: if two pairs
    // share a preimage, the replay above is only prevented by luck.
    let mut seen: Vec<(Context, SignedObjectKind, Vec<u8>)> = Vec::new();
    for &ctx in CONTEXTS {
        for &kind in ALL_SIGNED_OBJECT_KINDS {
            let p = preimage(ctx, kind);
            if let Some((oc, ok, _)) = seen.iter().find(|(_, _, prev)| *prev == p) {
                panic!("preimage collision: ({ctx:?}, {kind:?}) == ({oc:?}, {ok:?})");
            }
            seen.push((ctx, kind, p));
        }
    }
    assert_eq!(seen.len(), CONTEXTS.len() * ALL_SIGNED_OBJECT_KINDS.len());
}

// ---------------------------------------------------------------------------
// Coverage, derived from the registry rather than self-certified
// ---------------------------------------------------------------------------

/// Per-type tag checks, one per kind that has a `DomainSeparated` impl **in
/// this crate**.
///
/// Each entry calls production [`tag_matches_registry`], which compares the
/// type's `DOMAIN_TAG` associated constant against its registry row — so a tag
/// renamed in code but not in the registry, or emptied, fails here.
///
/// This list is hand-written, which is unavoidable: Rust cannot enumerate types
/// at runtime. What makes it non-self-certifying is
/// [`every_implemented_kind_is_tag_checked`], which fails when the registry
/// gains an implemented kind this list does not mention.
fn tag_checks() -> Vec<(SignedObjectKind, bool)> {
    use SignedObjectKind as K;
    use wrfcoin_forecast_score::blockchain::ForecastVerificationReceipt;
    use wrfcoin_shared_protocol::accounts::native_smart_accounts::{
        NativeAccountSubject, NativeSessionPossession, NativeSponsorSubject,
    };
    use wrfcoin_shared_protocol::blockchain::block::Block;
    use wrfcoin_shared_protocol::blockchain::snapshot_sync::SnapshotSyncPayload;
    use wrfcoin_shared_protocol::consensus::producer_set::ProducerSetChange;
    use wrfcoin_shared_protocol::consensus::validator_set::ValidatorSetChange;
    use wrfcoin_shared_protocol::cross_chain::types::BridgeMessage;
    use wrfcoin_shared_protocol::governance::proposal::Proposal;
    use wrfcoin_shared_protocol::governance::faucet_sink_pop::FaucetSinkPossession;
    use wrfcoin_shared_protocol::governance::schedule_extension::FeatureScheduleExtension;
    use wrfcoin_shared_protocol::governance::upgrade_notice::UpgradeNotice;
    use wrfcoin_shared_protocol::security::slashing::SlashingReportPayload;
    use wrfcoin_shared_protocol::weather::validation::WeatherReading;

    vec![
        (
            K::Transaction,
            tag_matches_registry::<Transaction>(K::Transaction),
        ),
        (
            K::BlockHeader,
            tag_matches_registry::<Block>(K::BlockHeader),
        ),
        (
            K::SlashingEvidence,
            tag_matches_registry::<SlashingReportPayload>(K::SlashingEvidence),
        ),
        (
            K::SnapshotPayload,
            tag_matches_registry::<SnapshotSyncPayload>(K::SnapshotPayload),
        ),
        (
            K::WeatherObservation,
            tag_matches_registry::<WeatherReading>(K::WeatherObservation),
        ),
        (
            K::OracleAttestation,
            tag_matches_registry::<OracleAttestationPayload>(K::OracleAttestation),
        ),
        (
            K::AttestationEnvelope,
            tag_matches_registry::<CalibrationAttestationPayload>(K::AttestationEnvelope),
        ),
        (
            K::GovernanceProposal,
            tag_matches_registry::<Proposal>(K::GovernanceProposal),
        ),
        (
            K::GovernanceVote,
            tag_matches_registry::<GovernanceVotePayload>(K::GovernanceVote),
        ),
        (
            K::BridgeMessage,
            tag_matches_registry::<BridgeMessage>(K::BridgeMessage),
        ),
        (
            K::ComputeReceipt,
            tag_matches_registry::<ComputeReceipt>(K::ComputeReceipt),
        ),
        (
            K::ForecastVerificationReceipt,
            tag_matches_registry::<ForecastVerificationReceipt>(K::ForecastVerificationReceipt),
        ),
        // #2227 introduced `ValidatorSetChange` and flipped its registry row
        // Exception -> Implemented, which brings it under this suite's coverage
        // requirement (`every_implemented_kind_is_tag_checked`).
        (
            K::ValidatorSetChange,
            tag_matches_registry::<ValidatorSetChange>(K::ValidatorSetChange),
        ),
        (
            K::ProducerSetChange,
            tag_matches_registry::<ProducerSetChange>(K::ProducerSetChange),
        ),
        // #3094 introduced the governance-signed `UpgradeNotice` type with a
        // `DomainSeparated` impl, bringing it under this suite's coverage
        // requirement (`every_implemented_kind_is_tag_checked`). #3148 covered
        // its canonical-encoding registry row; this is the sibling tag-check
        // site that #3148 did not reach.
        (
            K::UpgradeNotice,
            tag_matches_registry::<UpgradeNotice>(K::UpgradeNotice),
        ),
        // #3277: the governance-signed feature-schedule extension, its own
        // tag so an UpgradeNotice signature can never verify as one.
        (
            K::FeatureScheduleExtension,
            tag_matches_registry::<FeatureScheduleExtension>(K::FeatureScheduleExtension),
        ),
        // #3361 F3: the faucet-key proof of possession, its own tag so no
        // other signature by the faucet key verifies as one.
        (
            K::FaucetSinkPossession,
            tag_matches_registry::<FaucetSinkPossession>(K::FaucetSinkPossession),
        ),
        (
            K::NativeAccountSubject,
            tag_matches_registry::<NativeAccountSubject>(K::NativeAccountSubject),
        ),
        (
            K::NativeSponsorSubject,
            tag_matches_registry::<NativeSponsorSubject>(K::NativeSponsorSubject),
        ),
        (
            K::NativeSessionPossession,
            tag_matches_registry::<NativeSessionPossession>(K::NativeSessionPossession),
        ),
        (
            K::SlashEvidenceReport,
            tag_matches_registry::<
                wrfcoin_shared_protocol::security::slash_evidence::SlashEvidenceReport,
            >(K::SlashEvidenceReport),
        ),
        (
            K::NodeServiceReceipt,
            tag_matches_registry::<
                wrfcoin_shared_protocol::consensus::node_service_receipt::NodeServiceReceipt,
            >(K::NodeServiceReceipt),
        ),
        (K::NodeReceiptSubmissionAuthorization, tag_matches_registry::<wrfcoin_shared_protocol::consensus::node_receipt_registry::NodeReceiptSubmissionAuthorization>(K::NodeReceiptSubmissionAuthorization)),
        (K::NodeServiceTicket, tag_matches_registry::<wrfcoin_shared_protocol::consensus::node_share_contracts::NodeServiceTicket>(K::NodeServiceTicket)),
        (K::NodeSettlementEndorsement, tag_matches_registry::<wrfcoin_shared_protocol::consensus::node_share_contracts::NodeSettlementEndorsement>(K::NodeSettlementEndorsement)),
        (K::NodeBondAuthorization, tag_matches_registry::<wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAuthorization>(K::NodeBondAuthorization)),
        (K::NodeBondPossession, tag_matches_registry::<wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondPossession>(K::NodeBondPossession)),
    ]
}

#[test]
fn every_implemented_kind_is_tag_checked() {
    // The property the previous `assert_eq!(OBJECT_KINDS.len(), 7)` could not
    // have: coverage is compared against the REAL registry, so registering a
    // new signed type with an impl breaks this suite until it is covered.
    let checked: BTreeSet<SignedObjectKind> = tag_checks().into_iter().map(|(k, _)| k).collect();

    let mut missing = Vec::new();
    for &kind in ALL_SIGNED_OBJECT_KINDS {
        let e = entry(kind).unwrap_or_else(|| panic!("{kind:?} has no registry row"));
        if e.coverage.has_impl() && !checked.contains(&kind) {
            missing.push((kind, e.rust_type));
        }
    }

    assert!(
        missing.is_empty(),
        "these registered kinds have a DomainSeparated impl but no tag check in \
         this suite, so a tag change on them would go unnoticed: {missing:?}. \
         Add them to `tag_checks()`."
    );
}

#[test]
fn every_tag_check_passes() {
    for (kind, ok) in tag_checks() {
        assert!(
            ok,
            "{kind:?}: the type's DOMAIN_TAG does not match its registry row — \
             a tag was renamed in one place and not the other"
        );
    }
}

#[test]
fn the_matrix_covers_the_whole_registry() {
    // The matrix iterates ALL_SIGNED_OBJECT_KINDS directly, so this asserts the
    // registry and the inventory slice agree rather than pinning a literal.
    assert_eq!(
        ALL_SIGNED_OBJECT_KINDS.len(),
        SIGNED_OBJECT_REGISTRY.len(),
        "ALL_SIGNED_OBJECT_KINDS and SIGNED_OBJECT_REGISTRY disagree — one was \
         updated without the other, and the matrix would silently skip a kind"
    );
    for &kind in ALL_SIGNED_OBJECT_KINDS {
        assert!(entry(kind).is_some(), "{kind:?} has no registry row");
    }
}

// ---------------------------------------------------------------------------
// The typed production path
// ---------------------------------------------------------------------------

fn a_transaction() -> Transaction {
    Transaction::new(Vec::new(), Vec::new())
}

fn a_governance_vote() -> GovernanceVotePayload {
    GovernanceVotePayload {
        proposal_id: "prop-1".to_string(),
        approve: true,
        voting_power: Some(1),
        signature: None,
        signer_public_key: None,
    }
}

/// Every `DomainSeparated` impl builds its preimage with `domain_preimage`, so
/// the tag is the first length-prefixed field. Asserting that on a real
/// instance catches a type that hand-rolls its preimage and forgets the tag —
/// which the registry check alone cannot see, because the associated constant
/// would still be correct.
fn assert_preimage_is_tagged<T: DomainSeparated>(obj: &T, what: &str) {
    let mut expected_prefix = Vec::new();
    push_length_prefixed(&mut expected_prefix, T::DOMAIN_TAG);
    assert!(
        obj.signing_preimage().starts_with(&expected_prefix),
        "{what}: signing_preimage() does not begin with its length-prefixed \
         DOMAIN_TAG — this type is not domain separated in production, whatever \
         the registry says"
    );
}

#[test]
fn production_preimages_begin_with_their_domain_tag() {
    assert_preimage_is_tagged(&a_transaction(), "Transaction");
    assert_preimage_is_tagged(&a_governance_vote(), "GovernanceVotePayload");
}

#[test]
fn a_governance_vote_signature_does_not_verify_as_a_transaction() {
    // Driven entirely through the production signer and verifier: no local
    // preimage, no raw `PrivateKey::sign`.
    let (sk, pk) = signing_key();
    let vote = a_governance_vote();
    let tx = a_transaction();

    let sig = sign_domain(&sk, &vote);

    assert!(
        verify_domain(&pk, &vote, &sig),
        "a governance vote must verify as itself, or the negative below is vacuous"
    );
    assert!(
        !verify_domain(&pk, &tx, &sig),
        "REPLAY: a governance vote signature verified as a transaction through \
         the production verifier. This is a security finding."
    );
}

#[test]
fn a_transaction_signature_does_not_verify_as_a_governance_vote() {
    let (sk, pk) = signing_key();
    let tx = a_transaction();
    let vote = a_governance_vote();

    let sig = sign_domain(&sk, &tx);

    assert!(verify_domain(&pk, &tx, &sig));
    assert!(
        !verify_domain(&pk, &vote, &sig),
        "REPLAY: a transaction signature verified as a governance vote through \
         the production verifier. This is a security finding."
    );
}

// ---------------------------------------------------------------------------
// Vacuity guard — the concrete production edit, reproduced
// ---------------------------------------------------------------------------

#[test]
fn removing_domain_separation_from_production_breaks_this_suite() {
    // The previous version of this test signed one untagged preimage, verified
    // that same preimage, and compared it to itself. It demonstrated the model,
    // not the barrier.
    //
    // This reproduces the exact production edit it exists to catch — deleting
    // the tag line from `crypto::domain_signing::domain_preimage`:
    //
    //     pub fn domain_preimage(tag: &[u8], canonical_body: &[u8]) -> Vec<u8> {
    //         let mut buf = Vec::new();
    //     -   push_length_prefixed(&mut buf, tag);
    //         push_length_prefixed(&mut buf, canonical_body);
    //         buf
    //     }
    //
    // and asserts that with the tag gone the matrix's own inputs collapse, so
    // `full_replay_matrix_rejects_every_cross_context_verification` above cannot
    // still pass.
    fn untagged_preimage(context: Context, _kind: SignedObjectKind) -> Vec<u8> {
        let mut buf = Vec::new();
        push_length_prefixed(&mut buf, &body_for(context));
        buf
    }

    let ctx = Context::Mainnet;
    let mut collisions = 0usize;
    for &a in ALL_SIGNED_OBJECT_KINDS {
        for &b in ALL_SIGNED_OBJECT_KINDS {
            if a != b && untagged_preimage(ctx, a) == untagged_preimage(ctx, b) {
                collisions += 1;
            }
        }
    }

    let n = ALL_SIGNED_OBJECT_KINDS.len();
    assert_eq!(
        collisions,
        n * n - n,
        "with the domain tag removed, EVERY distinct pair of object kinds must \
         share a preimage — if not, this suite is measuring something other \
         than domain separation"
    );

    // And a signature over one untagged kind verifies as every other, which is
    // the assertion the matrix makes and would lose.
    let (sk, pk) = signing_key();
    let sig = sk.sign(&untagged_preimage(ctx, SignedObjectKind::Transaction));
    for &other in ALL_SIGNED_OBJECT_KINDS {
        assert!(
            pk.verify(&untagged_preimage(ctx, other), &sig),
            "untagged: a Transaction signature must verify as {other:?}"
        );
    }

    // With production tags restored, the same content no longer collides.
    assert_ne!(
        preimage(ctx, SignedObjectKind::Transaction),
        preimage(ctx, SignedObjectKind::GovernanceVote)
    );
}

// ---------------------------------------------------------------------------
// Named regressions for historically dangerous pairings
// ---------------------------------------------------------------------------
//
// These are covered by the matrix. They exist separately because a named,
// greppable test explains *why* the pairing matters — a matrix failure names a
// pair, not a threat.

#[test]
fn wallet_login_signature_is_not_a_transfer() {
    // The highest-value pairing in the system: a login challenge is signed by a
    // user's spending key on request. If those bytes can be reinterpreted as a
    // transfer, an attacker who can get a victim to "log in" can move funds.
    //
    // `WalletLoginChallenge` is a reserved tag with no Rust type yet (#2228), so
    // this drives it through the registry tag rather than a typed instance —
    // which is exactly why the matrix is registry-driven.
    let (sk, pk) = signing_key();
    let login = sign(
        &sk,
        Context::Mainnet,
        SignedObjectKind::WalletLoginChallenge,
    );

    assert!(verifies(
        &pk,
        Context::Mainnet,
        SignedObjectKind::WalletLoginChallenge,
        &login
    ));
    assert!(
        !verifies(&pk, Context::Mainnet, SignedObjectKind::Transaction, &login),
        "a wallet login signature must never verify as a transaction"
    );
}

#[test]
fn testnet_transfer_is_not_valid_on_mainnet() {
    // Testnet keys are handled casually and testnet signatures are freely
    // observable. If they carry to mainnet, the testnet is an oracle for
    // mainnet-valid signatures.
    let (sk, pk) = signing_key();
    let k = SignedObjectKind::Transaction;
    let testnet_transfer = sign(&sk, Context::Testnet, k);

    assert!(verifies(&pk, Context::Testnet, k, &testnet_transfer));
    assert!(
        !verifies(&pk, Context::Mainnet, k, &testnet_transfer),
        "a testnet transfer must never verify on mainnet"
    );
    assert!(
        !verifies(&pk, Context::OtherWrfNetwork, k, &testnet_transfer),
        "a testnet transfer must never verify on another WRF network"
    );
}

#[test]
fn governance_vote_is_not_a_finality_vote() {
    // Both are "votes" and both are signed by validators. A governance vote
    // reinterpreted as a finality vote is an unauthorised finality claim.
    //
    // #2231's review corrected `FinalityVote`'s binding from the unsigned
    // `consensus::finality::FinalitySignal` to the live
    // `consensus_engines::selection::bft_finality::FinalityVote`. That type
    // lives in a crate depending on shared-protocol (`CrossCrate`, #2249), so it
    // cannot be constructed here — the registry tag is the right surface, and
    // reading it from the registry is what makes the corrected binding testable
    // at all.
    let (sk, pk) = signing_key();
    let governance = sign(&sk, Context::Mainnet, SignedObjectKind::GovernanceVote);

    assert!(verifies(
        &pk,
        Context::Mainnet,
        SignedObjectKind::GovernanceVote,
        &governance
    ));
    assert!(
        !verifies(
            &pk,
            Context::Mainnet,
            SignedObjectKind::FinalityVote,
            &governance
        ),
        "a governance vote must never verify as a finality vote"
    );
}

#[test]
fn attestation_envelope_is_not_a_transaction() {
    // The #2065 shape, as a permanent regression — and here through the typed
    // production path, since both types exist.
    let (sk, pk) = signing_key();
    let attestation = sign(&sk, Context::Mainnet, SignedObjectKind::AttestationEnvelope);

    assert!(verifies(
        &pk,
        Context::Mainnet,
        SignedObjectKind::AttestationEnvelope,
        &attestation
    ));
    assert!(
        !verifies(
            &pk,
            Context::Mainnet,
            SignedObjectKind::Transaction,
            &attestation
        ),
        "an attestation envelope must never verify as a transaction (#2065)"
    );
}

#[test]
fn suite_runs_in_the_default_test_path_in_under_a_second() {
    // Acceptance criterion. A replay suite that is slow enough to be marked
    // `#[ignore]` is a replay suite that stops running. The matrix is now 18
    // kinds rather than 7, so this is a real budget check, not a formality.
    let started = Instant::now();
    let (sk, pk) = signing_key();
    let pairs = all_pairs();
    for &(ctx_a, kind_a) in &pairs {
        let sig = sign(&sk, ctx_a, kind_a);
        for &(ctx_b, kind_b) in &pairs {
            let _ = verifies(&pk, ctx_b, kind_b, &sig);
        }
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed.as_secs() < 1,
        "full matrix took {elapsed:?}, over the one-second budget"
    );
}
