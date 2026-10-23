//! Block-producer authority resolution + the enforcement gate (#2290, Option B).
//!
//! Authentication ("did the holder of `producer_pubkey` sign this block?") is
//! [`crate::consensus::block_producer_auth::verify_block_producer_on`]. This
//! module answers **authority**: is that key allowed to produce at this height,
//! and does the block pay the producer's stable payout address?
//!
//! Owner decisions (locked): PARAM-1 governance-managed on-chain set (dedicated
//! [`ProducerSetChange`], §R1), PARAM-2 enforce-at-genesis (every height ≥ 1;
//! genesis is pinned outside this gate, §4), §R2 stable `payout_address`
//! decoupled from the rotatable signing key.
//!
//! The resolver reads the on-chain [`ProducerSetChange`] history
//! (`State::producer_set_history`); the authorised set at height `H` is the
//! change with the greatest `effective_height <= H`. Construction is fallible and
//! fail-closed ([`GovernanceProducers::try_from_history`]) — an invalid history is
//! an error, never a silently-normalized or empty set.

use crate::blockchain::block::Block;
use crate::consensus::block_producer_auth::{ProducerAuthError, verify_block_producer_on};
use crate::consensus::producer_set::{
    ProducerMember, ProducerSetChange, ProducerSetError, validate_producer_set_history,
};
use crate::crypto::chain_binding::{
    ChainIdentity, SigningProtocolVersion, protocol_version_at,
};

/// Whether producer-authentication enforcement applies at a height and, if so,
/// the resolved authorised members there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProducerAuthorityDecision {
    /// Enforcement is not active at this height (not reachable under
    /// enforce-at-genesis; retained for the interface's generality).
    NotEnforced,
    /// The authorised producer committee at this height. The block's
    /// authenticated key must match a member, and the coinbase must pay that
    /// member's `payout_address`.
    Enforced(Vec<ProducerMember>),
}

/// Resolves the authorised producers at a height from validated on-chain history.
pub trait ProducerAuthorityPolicy: Send + Sync {
    fn authorized_producers_at(&self, height: u64) -> ProducerAuthorityDecision;
}

/// Governance-managed set resolver (PARAM-1). Holds only a VALIDATED history, so
/// resolution is total and never silently drops a malformed member.
#[derive(Clone, Debug, Default)]
pub struct GovernanceProducers {
    /// Validated set-change history in application order (strictly-increasing
    /// `effective_height`).
    validated: Vec<ProducerSetChange>,
}

impl GovernanceProducers {
    /// The initial change genesis seeds — `epoch 0`, `effective_height 0`. This is
    /// the single element Alice writes into `State::producer_set_history`.
    ///
    /// `effective_height 0` is a field on the *change*, not a block: there is no
    /// block at height 0 (height 0 is the empty-chain state, the first block is
    /// height 1). Effective-at-0 means the set is already authoritative when
    /// block 1 — the first block that can be gated — is validated.
    pub fn genesis_change(members: Vec<ProducerMember>) -> ProducerSetChange {
        ProducerSetChange::new(0, 0, members)
    }

    /// Build a resolver from the on-chain history (as read from `State`).
    /// FALLIBLE + fail-closed: [`validate_producer_set_history`] rejects a
    /// malformed / non-monotonic / out-of-order / duplicate / non-canonical log.
    pub fn try_from_history(changes: &[ProducerSetChange]) -> Result<Self, ProducerSetError> {
        validate_producer_set_history(changes)?;
        Ok(Self { validated: changes.to_vec() })
    }
}

impl ProducerAuthorityPolicy for GovernanceProducers {
    fn authorized_producers_at(&self, height: u64) -> ProducerAuthorityDecision {
        // `validated` is in application order with strictly-increasing
        // effective_height, so the active change is the last one whose
        // effective_height <= height.
        let members = self
            .validated
            .iter()
            .rfind(|c| c.effective_height <= height)
            .map(|c| c.members.clone())
            .unwrap_or_default();
        ProducerAuthorityDecision::Enforced(members)
    }
}

/// Resolve the authorised producer set for a particular block height from the
/// on-chain history.
///
/// This is the boot and production-side entry point for height-sensitive
/// checks. Callers must supply the height they are about to act on; in
/// particular, a node boot check must pass `current_height + 1`, not the
/// height of the newest persisted block. The history is validated before
/// resolution so a malformed future change cannot be silently ignored.
pub fn resolve_authorized_producers_at(
    producer_set_history: &[ProducerSetChange],
    height: u64,
) -> Result<Vec<ProducerMember>, String> {
    if producer_set_history.is_empty() {
        return Ok(Vec::new());
    }
    let policy = GovernanceProducers::try_from_history(producer_set_history)
        .map_err(|e| format!("invalid producer_set_history in state (#2290): {e:?}"))?;
    match policy.authorized_producers_at(height) {
        ProducerAuthorityDecision::NotEnforced => Ok(Vec::new()),
        ProducerAuthorityDecision::Enforced(members) => Ok(members),
    }
}

/// Why a block's producer failed the authority check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProducerAuthorityError {
    /// The producer signature did not authenticate (unsigned / malformed key /
    /// bad signature).
    Unauthenticated(ProducerAuthError),
    /// Authenticated, but the key is not in the authorised set at this height.
    NotAuthorized,
    /// Authenticated + authorised, but the coinbase does not pay the member's
    /// stable `payout_address` (§R2).
    WrongPayoutAddress,
}

/// The enforcement gate: authenticate the producer, resolve the authorised
/// member by key, and require the coinbase to pay that member's `payout_address`.
///
/// **NOTE (§4):** no `block.height == 0` exemption — the height field is
/// attacker-controlled and the untrusted acceptance validators do not establish
/// expected height. Genesis is pinned by deterministic init and never flows
/// through this gate (gossip handles received blocks; IBD accepts only
/// `chain.height + 1 >= 1`), so the gate enforces on every block it sees.
///
/// The wiring maps `Ok(())` → accept, any `Err` → reject the block (`Ok(false)`).
pub fn check_block_producer_authority(
    block: &Block,
    policy: &dyn ProducerAuthorityPolicy,
    identity: &ChainIdentity,
    version: SigningProtocolVersion,
) -> Result<(), ProducerAuthorityError> {
    let members = match policy.authorized_producers_at(block.height) {
        ProducerAuthorityDecision::NotEnforced => return Ok(()),
        ProducerAuthorityDecision::Enforced(m) => m,
    };

    // 1. Authenticate the producer signature.
    let key = verify_block_producer_on(block, identity, version)
        .map_err(ProducerAuthorityError::Unauthenticated)?;
    let key_bytes: [u8; 32] = key
        .as_bytes()
        .try_into()
        .map_err(|_| ProducerAuthorityError::Unauthenticated(ProducerAuthError::MalformedKey))?;

    // 2. Authority: the authenticated key must be an authorised member here.
    let Some(member) = members.iter().find(|m| m.public_key == key_bytes) else {
        return Err(ProducerAuthorityError::NotAuthorized);
    };

    // 3. Payout binding (§R2), after the coinbase structure is established: the
    //    coinbase recipient must be the member's STABLE payout_address (not the
    //    signer-derived address), so signing-key rotation does not move payouts.
    let coinbase_recipient = block
        .transactions
        .first()
        .and_then(|cb| cb.outputs.first())
        .map(|o| o.address.clone())
        .unwrap_or_default();
    if coinbase_recipient == member.payout_address {
        Ok(())
    } else {
        Err(ProducerAuthorityError::WrongPayoutAddress)
    }
}

/// The ONE shared gate both untrusted acceptance boundaries call — gossip
/// (`pop_adapter::validate_block`) and IBD (`sync.rs::validate_block`) — with the
/// SAME `chain.state.producer_set_history` snapshot and the active chain identity
/// and signing version. Wiring both to this single function is what stops the two
/// call sites drifting (Dan §1).
///
/// Returns `Ok(())` to accept, `Err(reason)` to reject the block. Fail-closed: an
/// invalid on-chain history is itself a rejection.
pub fn verify_block_producer_authorized(
    block: &Block,
    producer_set_history: &[ProducerSetChange],
) -> Result<(), String> {
    // Activation: enforcement is active IFF the chain has a seeded producer set.
    // The fresh-genesis testnet seeds it into genesis *state* (no block at height
    // 0 exists — height 0 is the empty chain), so enforcement is live from block 1,
    // the first block that reaches this gate (enforce-at-genesis, PARAM-2). A
    // chain with NO producer set is one
    // that never configured producer-auth (legacy / in-process tests) — not
    // enforced. The set lives in `state_root`, so an attacker cannot "unseed" a
    // configured chain to dodge the gate.
    if producer_set_history.is_empty() {
        return Ok(());
    }
    let policy = GovernanceProducers::try_from_history(producer_set_history)
        .map_err(|e| format!("invalid producer_set_history in state (#2290): {e:?}"))?;
    check_block_producer_authority(
        block,
        &policy,
        ChainIdentity::active(),
        protocol_version_at(block.height),
    )
    .map_err(|e| format!("producer authority (#2290): {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::{Transaction, TransactionOutput};
    use crate::common::types::Address;
    use crate::consensus::block_producer_auth::sign_block_producer;
    use crate::crypto::hash::Hash;
    use crate::crypto::signature::{PrivateKey, PublicKey};
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

    /// Canonical payout `Address` (0x-hex) derived from a key — the coinbase form
    /// the mining route accepts (NOT `pk.to_address()`'s wrf form).
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

    /// A height-`h` block whose coinbase pays `recipient`, signed by `signer` (or
    /// left unsigned when `signer` is None).
    fn block_paying(h: u64, recipient: &str, signer: Option<&PrivateKey>) -> Block {
        let mut coinbase = Transaction::new(vec![], vec![TransactionOutput {
            amount: 5000,
            address: recipient.to_string(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }]);
        coinbase.set_id();
        let mut b = Block::new(3, Hash::default(), vec![coinbase], 1, h);
        b.timestamp = DateTime::<Utc>::from_timestamp_millis(1_700_000_000_000).unwrap();
        if let Some(sk) = signer {
            let att = sign_block_producer(&b, sk, &identity(), SigningProtocolVersion::V2);
            b.producer_pubkey = att.producer_pubkey;
            b.producer_sig = att.producer_sig.to_vec();
        }
        b
    }

    fn seeded_policy(members: Vec<ProducerMember>) -> GovernanceProducers {
        GovernanceProducers::try_from_history(&[GovernanceProducers::genesis_change(members)])
            .expect("valid seeded history")
    }

    #[test]
    fn resolver_selects_the_set_at_the_requested_future_height() {
        let (_genesis_sk, genesis_pk) = keypair(1);
        let (_next_sk, next_pk) = keypair(2);
        let history = vec![
            ProducerSetChange::new(0, 0, vec![member_of(&genesis_pk, "genesis")]),
            ProducerSetChange::new(1, 8, vec![member_of(&next_pk, "next")]),
        ];

        assert_eq!(
            resolve_authorized_producers_at(&history, 7).unwrap()[0].producer_id,
            "genesis"
        );
        assert_eq!(
            resolve_authorized_producers_at(&history, 8).unwrap()[0].producer_id,
            "next"
        );
    }

    #[test]
    fn accepts_authorized_member_paying_its_payout_address() {
        let (sk, pk) = keypair(1);
        let member = member_of(&pk, "m");
        let policy = seeded_policy(vec![member.clone()]);
        let block = block_paying(1, &member.payout_address, Some(&sk));
        assert_eq!( // secret-leak-ok: public protocol enum; result carries only unit/error variants
            check_block_producer_authority(&block, &policy, &identity(), SigningProtocolVersion::V2),
            Ok(())
        );
    }

    #[test]
    fn rejects_unsigned_block() {
        let (_, pk) = keypair(2);
        let member = member_of(&pk, "m");
        let policy = seeded_policy(vec![member.clone()]);
        let block = block_paying(1, &member.payout_address, None);
        assert_eq!( // secret-leak-ok: public protocol enum; result carries only unit/error variants
            check_block_producer_authority(&block, &policy, &identity(), SigningProtocolVersion::V2),
            Err(ProducerAuthorityError::Unauthenticated(ProducerAuthError::Unsigned))
        );
    }

    #[test]
    fn rejects_forged_height0_no_exemption() {
        let (_, pk) = keypair(3);
        let member = member_of(&pk, "m");
        let policy = seeded_policy(vec![member.clone()]);
        // §4: a forged unsigned height-0 block is NOT exempt.
        let forged = block_paying(0, &member.payout_address, None);
        assert_eq!( // secret-leak-ok: public protocol enum; result carries only unit/error variants
            check_block_producer_authority(&forged, &policy, &identity(), SigningProtocolVersion::V2),
            Err(ProducerAuthorityError::Unauthenticated(ProducerAuthError::Unsigned))
        );
    }

    #[test]
    fn rejects_non_member_even_paying_its_own_address() {
        let (member_sk, member_pk) = keypair(4);
        let (stranger_sk, stranger_pk) = keypair(5);
        // Authorised set has the member; the stranger signs + pays its OWN payout
        // → rejected SOLELY for non-membership.
        let policy = seeded_policy(vec![member_of(&member_pk, "m")]);
        let _ = member_sk;
        let block = block_paying(1, &payout_of(&stranger_pk), Some(&stranger_sk));
        assert_eq!( // secret-leak-ok: public protocol enum; result carries only unit/error variants
            check_block_producer_authority(&block, &policy, &identity(), SigningProtocolVersion::V2),
            Err(ProducerAuthorityError::NotAuthorized)
        );
    }

    #[test]
    fn rejects_member_paying_wrong_address() {
        // Authorised member signs, but the coinbase pays a DIFFERENT address than
        // its stable payout_address → WrongPayoutAddress (§R2).
        let (sk, pk) = keypair(6);
        let member = member_of(&pk, "m");
        let policy = seeded_policy(vec![member]);
        let (_, other_pk) = keypair(7);
        let block = block_paying(1, &payout_of(&other_pk), Some(&sk));
        assert_eq!( // secret-leak-ok: public protocol enum; result carries only unit/error variants
            check_block_producer_authority(&block, &policy, &identity(), SigningProtocolVersion::V2),
            Err(ProducerAuthorityError::WrongPayoutAddress)
        );
    }

    /// #2085 class — absence collapsed into emptiness.
    ///
    /// When NO change is effective yet (every `effective_height` is above the
    /// block being validated), `next_back()` yields `None` and
    /// `unwrap_or_default()` turns that into an EMPTY member list — which is
    /// then returned as `Enforced`. "No committee is defined here" and "the
    /// committee here is empty" are different facts; this collapses them.
    ///
    /// `ProducerAuthorityDecision::NotEnforced` exists and can express the
    /// distinction. It is simply not used on this path.
    #[test]
    fn no_change_effective_yet_collapses_absence_into_an_empty_enforced_set() {
        let (_, a) = keypair(8);
        let policy = GovernanceProducers::try_from_history(&[ProducerSetChange::new(
            0,
            100,
            vec![member_of(&a, "a")],
        )])
        .expect("valid history");

        assert_eq!(
            policy.authorized_producers_at(50),
            ProducerAuthorityDecision::Enforced(Vec::new()),
            "absence of an effective change is reported as an enforced EMPTY committee"
        );
    }

    /// The consequence, stated as behaviour rather than inference: below the
    /// first effective height, a block signed by the very member the history
    /// names is rejected. Fail-CLOSED here, which is the safe direction — but it
    /// is incidental, not chosen, because the empty set is never distinguished
    /// from an absent one.
    #[test]
    fn below_the_first_effective_height_even_the_named_producer_is_rejected() {
        let (sk, a) = keypair(8);
        let policy = GovernanceProducers::try_from_history(&[ProducerSetChange::new(
            0,
            100,
            vec![member_of(&a, "a")],
        )])
        .expect("valid history");

        let block = block_paying(50, &payout_of(&a), Some(&sk));
        assert_eq!(
            check_block_producer_authority(
                &block,
                &policy,
                &identity(),
                SigningProtocolVersion::V2
            ),
            Err(ProducerAuthorityError::NotAuthorized),
            "a correctly-signed block by the named member is refused below the \
             first effective height"
        );
    }

    #[test]
    fn resolver_picks_active_change_at_height() {
        let (_, a) = keypair(8);
        let (_, b) = keypair(9);
        let policy = GovernanceProducers::try_from_history(&[
            ProducerSetChange::new(0, 0, vec![member_of(&a, "a")]),
            ProducerSetChange::new(1, 100, vec![member_of(&b, "b")]),
        ])
        .expect("valid history");
        assert_eq!(
            policy.authorized_producers_at(50),
            ProducerAuthorityDecision::Enforced(vec![member_of(&a, "a")])
        );
        assert_eq!(
            policy.authorized_producers_at(100),
            ProducerAuthorityDecision::Enforced(vec![member_of(&b, "b")])
        );
    }
}

/// The lowest persisted block that carries a producer signature, if any.
///
/// # Why this exists: "signed blocks but no producer set" must be fail-closed
///
/// Producer-authority enforcement activates *iff* the chain has a seeded
/// producer set, so an empty `producer_set_history` means "this chain never
/// configured producer auth" and is deliberately unenforced — that is what keeps
/// legacy and dev chains working.
///
/// But an empty history is reachable a second way, and it is not benign: a node
/// recovered by checkpoint-less **WAL replay** loses the set, because
/// `producer_set_history` is written into genesis `State` by
/// `State::seed_genesis_producer_set` and is carried by no `StateTransition`, so
/// replay has nothing to reconstruct it from (#2501). `main.rs` re-seeds only on
/// its fresh-genesis branch. Such a node holds signed blocks and an empty set,
/// and every producer-authority gate then reads it as an unconfigured chain —
/// enforcement silently off.
///
/// **Signed blocks are the discriminator.** A chain that was never configured has
/// unsigned blocks; a chain that lost its set has blocks its own producer signed.
/// The combination "empty set + signed blocks" is therefore not a legitimate
/// state and must refuse rather than start unenforced.
///
/// At V2 the tip state-root commitment check (#2219) already refuses this state,
/// because the set is folded into `state_root`. This is the backstop for the case
/// that check cannot see: **V1**, where the commitment is absent by design (V1
/// hashes are frozen) and the gate returns `Ok` for any version below V2. A
/// pre-cut chain that seeds a producer set therefore has the loss with none of
/// the detection — this closes that window.
///
/// "Signed" uses the same predicate as the verify boundary
/// ([`ProducerAttestation::from_block`] + [`ProducerAttestation::is_unsigned`]),
/// so a block this reports as signed is exactly one the gate would try to
/// authenticate. A malformed signature (not 64 bytes) reads as unsigned, which is
/// the conservative direction here: it cannot manufacture a refusal.
///
/// Returns the LOWEST such height so an operator is pointed at where the chain
/// began carrying signatures rather than at its tip.
pub fn lowest_signed_block<'a, I>(blocks: I) -> Option<u64>
where
    I: IntoIterator<Item = (&'a u64, &'a Block)>,
{
    use crate::consensus::block_producer_auth::ProducerAttestation;
    blocks
        .into_iter()
        .filter(|(_, b)| !ProducerAttestation::from_block(b).is_unsigned())
        .map(|(h, _)| *h)
        .min()
}

/// Boot-time backstop: verify EVERY retained block against the persisted
/// producer-set history (#2290).
///
/// `Blockchain::from_persisted` is a pure constructor and the storage loader
/// checks only the tip state-root commitment, so a restart inherits whatever is
/// on disk. Gating the acceptance boundaries (gossip, IBD) and the production
/// path (`/blocks/mine`) is necessary but not sufficient — those cover only the
/// routes we enumerated. This is the backstop for the ones we did not, and for
/// a chain that acquired an unauthorized block while running an older binary
/// that lacked a gate.
///
/// Returns `Err((height, reason))` for the LOWEST offending block, so the
/// operator is pointed at where the fork began rather than at its tip, and
/// `Ok(n)` with the number of blocks actually checked.
///
/// Unenforced when the chain never configured a producer set (empty history),
/// exactly as [`verify_block_producer_authorized`] is, so this cannot brick a
/// legacy or dev chain.
pub fn verify_persisted_blocks_authorized<'a, I>(
    blocks: I,
    producer_set_history: &[ProducerSetChange],
) -> Result<usize, (u64, String)>
where
    I: IntoIterator<Item = (&'a u64, &'a Block)>,
{
    if producer_set_history.is_empty() {
        return Ok(0);
    }
    let mut checked = 0usize;
    let mut lowest: Option<(u64, String)> = None;
    for (height, block) in blocks {
        checked += 1;
        if let Err(reason) = verify_block_producer_authorized(block, producer_set_history) {
            match &lowest {
                Some((h, _)) if *h <= *height => {},
                _ => lowest = Some((*height, reason)),
            }
        }
    }
    match lowest {
        Some(bad) => Err(bad),
        None => Ok(checked),
    }
}
