//! BFT Finality Gadget with 2/3+ Supermajority Voting
//!
//! Implements a GRANDPA/Tendermint-style BFT finality gadget that tracks
//! validator votes on blocks and declares finality when a 2/3+ supermajority
//! of the validator set (by stake weight) has voted for the same block.
//!
//! Unlike the probabilistic finality in [`super::finality`], this module
//! provides **deterministic** finality: once a block is finalized, it cannot
//! be reverted without at least 1/3 of validators being byzantine.
//!
//! # Design
//!
//! - **`FinalityVote`**: A validator's attestation that a block should be finalized.
//! - **`BftFinalityGadget`**: Accumulates votes per block and checks whether
//!   the 2/3+ threshold has been reached.
//! - **`FinalityEvent`**: Events emitted when finality status changes, suitable
//!   for propagation over the p2p layer.
//!
//! # Integration
//!
//! The gadget is designed to sit alongside the existing consensus engine:
//!
//! 1. After a block is proposed and included in the chain, validators submit
//!    `FinalityVote`s via [`BftFinalityGadget::submit_vote`].
//! 2. The gadget checks whether the 2/3+ threshold is met.
//! 3. If finality is achieved, a [`FinalityEvent::BlockFinalized`] is returned
//!    for the p2p layer to propagate.
//!
//! # References
//!
//! - Issue #1160: Replace derived-formula finality signal with a real BFT finality gadget
//! - GRANDPA: <https://wiki.polkadot.network/docs/learn-consensus#finality-gadget-grandpa>
//! - Tendermint: <https://docs.tendermint.com/v0.34/introduction/what-is-tendermint.html>

#![allow(dead_code)]

use ed25519_dalek::{Signature as Ed25519Signature, Verifier, VerifyingKey};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use wrfcoin_shared_protocol::consensus::finality::{
    FinalitySignal, FinalitySignalSender, emit_finality_signal,
};
// #2249: imported unqualified so the source scan in
// `shared-protocol/tests/signed_type_inventory.rs` (which matches
// `impl DomainSeparated for T`) discovers this cross-crate impl.
use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;

// ---------------------------------------------------------------------------
// Tuning constants
// ---------------------------------------------------------------------------

/// Multiplier applied to the validator count to derive the pruning lag used by
/// the auto-prune path in [`BftFinalityGadget::submit_vote`] (#1970).
///
/// After a block is finalized at height `H`, vote/equivocation maps for heights
/// strictly below `H - (validator_count * PRUNE_LAG_MULTIPLIER)` are dropped.
/// Keeping a lag (rather than pruning right up to `H`) leaves a generous window
/// of recent, already-finalized heights intact so that:
///   - late-arriving or out-of-order votes for the just-finalized region are
///     still attributable for equivocation detection, and
///   - any consumer reading [`finalized_block_hash`](BftFinalityGadget::finalized_block_hash)
///     for recent heights still sees data.
///
/// A value of `2` means: with `N` validators, retain roughly `2N` heights of
/// finalized history. This bounds map growth to O(validator_count) instead of
/// growing without limit as the chain advances.
const PRUNE_LAG_MULTIPLIER: u64 = 2;

/// Hard floor on the pruning lag (in heights), independent of validator count.
///
/// Guards against pathologically small validator sets producing a tiny lag that
/// would prune heights still plausibly receiving votes. With a single-validator
/// set, `2 * 1 = 2`, so this floor of `64` dominates and keeps a comfortable
/// retention window.
const MIN_PRUNE_LAG: u64 = 64;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A validator's vote attesting that a particular block should be finalized.
///
/// # Committee binding (#2256)
///
/// `epoch` and `validator_set_hash` name the committee the vote was cast into,
/// and both are folded into the signed preimage ([`content_hash`](Self::content_hash)).
/// Without them a vote is a bare "validator V says height H should be block B"
/// with no statement about the set it belongs to, so it counts toward a quorum
/// computed from a *different* total power — the stale-vote gap TLA+ found in
/// #2099 (`NoStaleVoteCounted`). With them, a vote cast under one committee is
/// cryptographically distinct from the same content cast under another and is
/// rejected by [`BftFinalityGadget::submit_vote`] for a mismatched set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalityVote {
    /// Hash of the block being voted on.
    pub block_hash: [u8; 32],
    /// Height of the block being voted on.
    pub block_height: u64,
    /// Epoch (validator-set generation) the vote is cast under (#2256).
    #[serde(default)]
    pub epoch: u64,
    /// Hash of the ordered validator set the vote is cast under (#2256).
    /// See [`BftFinalityGadget::validator_set_hash`].
    #[serde(default)]
    pub validator_set_hash: [u8; 32],
    /// Identifier of the voting validator (e.g., address or public key hash).
    pub validator_id: String,
    /// Cryptographic signature over [`content_hash`](Self::content_hash).
    pub signature: Vec<u8>,
}

impl FinalityVote {
    /// Compute a deterministic hash of the vote contents (excluding signature)
    /// for deduplication and integrity checks.
    ///
    /// The `wrfcoin-finality-vote-v2|` domain separator reflects the #2256
    /// preimage change: `epoch` and `validator_set_hash` now bind the vote to
    /// its committee. Fixed-width fields precede the variable-length
    /// `validator_id` so the concatenation stays injective.
    pub fn content_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"wrfcoin-finality-vote-v2|");
        hasher.update(self.block_hash);
        hasher.update(self.block_height.to_be_bytes());
        hasher.update(self.epoch.to_be_bytes());
        hasher.update(self.validator_set_hash);
        hasher.update(self.validator_id.as_bytes());
        let result = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }
}

// #2249: `DomainSeparated` for the cross-crate signed type. The tag is reserved
// in `wrfcoin_shared_protocol::crypto::domain_registry` (CrossCrate row); coherence
// permits the impl here (foreign trait, local type). The trait's provided methods
// add the tag + chain id + network id + version framing, so `canonical_body`
// carries only the object's own bytes and excludes the signature.
impl DomainSeparated for FinalityVote {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.finality_vote.v1";

    fn canonical_body(&self) -> Vec<u8> {
        wrfcoin_shared_protocol::crypto::domain_impls::BodyBuilder::new()
            .bytes_field(&self.block_hash)
            .u64_field(self.block_height)
            .str_field(&self.validator_id)
            .finish()
    }
}

/// Events emitted by the BFT finality gadget for p2p propagation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinalityEvent {
    /// A new vote was received and recorded (but finality not yet reached).
    VoteReceived {
        block_hash: [u8; 32],
        block_height: u64,
        validator_id: String,
        /// Number of votes accumulated so far for this block.
        vote_count: usize,
        /// Total stake weight accumulated so far.
        accumulated_weight: u128,
        /// Total stake weight required for finality.
        required_weight: u128,
    },
    /// A block has been finalized (2/3+ supermajority reached).
    BlockFinalized {
        block_hash: [u8; 32],
        block_height: u64,
        /// Total stake weight of the votes that achieved finality.
        total_vote_weight: u128,
        /// Total stake weight of the full validator set.
        total_validator_weight: u128,
        /// Number of validators that voted.
        voter_count: usize,
    },
    /// A duplicate vote was rejected.
    DuplicateVoteRejected {
        block_hash: [u8; 32],
        block_height: u64,
        validator_id: String,
    },
}

/// A BFT finality certificate: the 2/3+1 supermajority vote set that
/// finalized a block. Emitted by [`BftFinalityGadget::submit_vote`] when the
/// accumulated stake weight first crosses the supermajority threshold.
///
/// The certificate is the persistable record of *why* a block is final — the
/// set of votes that justify it. Consumers use it for:
/// - Gossip propagation so nodes that missed individual votes can still
///   adopt the finality decision.
/// - Audit logs and post-mortem analysis of finality gadget behavior.
/// - Reorg-guard hardening beyond the bare `FinalitySignal` emission.
///
/// TODO(#1388): populate this struct in `submit_vote` when a block first
/// crosses the supermajority threshold, and surface it alongside the
/// `FinalityEvent::BlockFinalized` return value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalityCertificate {
    /// Hash of the finalized block.
    pub block_hash: [u8; 32],
    /// Height of the finalized block.
    pub block_height: u64,
    /// The exact votes (>= 2/3+1 by stake) that achieved finality.
    pub votes: Vec<FinalityVote>,
    /// Sum of stake weight across `votes`.
    pub total_vote_weight: u128,
    /// Sum of stake weight across the full validator set at finality time.
    pub total_validator_weight: u128,
}

/// Errors that can occur when submitting votes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BftFinalityError {
    /// The validator is not in the current validator set.
    #[error("Unknown validator: {0}")]
    UnknownValidator(String),

    /// The validator entry has no registered public key, so a verified
    /// vote path cannot authenticate this submission. Used by
    /// [`BftFinalityGadget::submit_vote_verified`].
    #[error("Validator {0} has no registered public key")]
    NoPublicKey(String),

    /// The signature attached to the vote does not verify against the
    /// validator's registered public key.
    #[error("Invalid signature from validator {0}")]
    InvalidSignature(String),

    /// The validator's registered public key bytes are not a valid
    /// ed25519 public key.
    #[error("Malformed public key for validator {0}")]
    MalformedPublicKey(String),

    /// The validator has already voted for this block height.
    #[error("Duplicate vote from validator {validator_id} at height {block_height}")]
    DuplicateVote {
        validator_id: String,
        block_height: u64,
    },

    /// The block has already been finalized.
    #[error("Block at height {0} is already finalized")]
    AlreadyFinalized(u64),

    /// Equivocation detected: validator voted for a different block at the same height.
    #[error(
        "Equivocation by validator {validator_id} at height {block_height}: \
         voted for different blocks"
    )]
    Equivocation {
        validator_id: String,
        block_height: u64,
    },

    /// A gossiped finality certificate was internally inconsistent or failed
    /// to prove a supermajority for its claimed block.
    #[error("Invalid finality certificate: {0}")]
    InvalidCertificate(String),

    /// The vote names a different committee than this gadget serves — its
    /// `epoch` / `validator_set_hash` do not match the active set (#2256). A
    /// vote is valid only under the committee it was cast in; counting it
    /// toward a quorum computed from a different total power is the stale-vote
    /// gap #2099's TLA+ found.
    #[error(
        "vote for validator {validator_id} is scoped to a different committee \
         (epoch {vote_epoch}/set {vote_set_hash} vs active epoch {active_epoch}/set {active_set_hash})"
    )]
    WrongCommittee {
        /// The validator whose vote was rejected.
        validator_id: String,
        /// Epoch the vote claimed.
        vote_epoch: u64,
        /// Validator-set hash the vote claimed (hex).
        vote_set_hash: String,
        /// Epoch this gadget serves.
        active_epoch: u64,
        /// Validator-set hash this gadget serves (hex).
        active_set_hash: String,
    },
}

// ---------------------------------------------------------------------------
// Validator set tracking
// ---------------------------------------------------------------------------

/// A validator's identity and stake weight for the finality gadget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalityValidator {
    /// Unique identifier (address / pubkey hash).
    pub id: String,
    /// Stake weight used for supermajority calculation.
    pub stake_weight: u64,
    /// 32-byte ed25519 public key used by
    /// [`BftFinalityGadget::submit_vote_verified`] to authenticate votes
    /// claiming this validator id (#1388). `None` means this validator's
    /// key is not registered with the gadget — the bare
    /// [`BftFinalityGadget::submit_vote`] path still works (used by the
    /// pre-#1388 tests), but the verified path returns
    /// [`BftFinalityError::NoPublicKey`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<[u8; 32]>,
}

// ---------------------------------------------------------------------------
// Vote accumulation per block
// ---------------------------------------------------------------------------

/// Accumulated votes for a single block.
#[derive(Debug, Clone)]
struct BlockVotes {
    block_hash: [u8; 32],
    block_height: u64,
    /// Map from validator_id -> FinalityVote.
    votes: FxHashMap<String, FinalityVote>,
    /// Running total of stake weight that has voted.
    accumulated_weight: u128,
    /// Whether this block has been finalized.
    finalized: bool,
}

impl BlockVotes {
    fn new(block_hash: [u8; 32], block_height: u64) -> Self {
        Self {
            block_hash,
            block_height,
            votes: FxHashMap::default(),
            accumulated_weight: 0,
            finalized: false,
        }
    }
}

// ---------------------------------------------------------------------------
// BFT Finality Gadget
// ---------------------------------------------------------------------------

/// BFT finality gadget that declares finality when 2/3+ of validators
/// (weighted by stake) have voted for the same block.
///
/// # Supermajority rule
///
/// Finality is declared when `accumulated_weight * 3 > total_weight * 2`,
/// i.e., strictly more than 2/3 of total validator stake has voted.
/// This avoids floating-point arithmetic entirely.
///
/// # Chain-layer wiring (#1216)
///
/// The gadget can be attached to a
/// [`FinalitySignalSender`](wrfcoin_shared_protocol::consensus::finality::FinalitySignalSender)
/// via [`BftFinalityGadget::with_finality_sink`] or
/// [`BftFinalityGadget::set_finality_sink`]. When this sender is present,
/// every time [`submit_vote`](Self::submit_vote) elevates a block to
/// finality the gadget emits a
/// [`FinalitySignal`](wrfcoin_shared_protocol::consensus::finality::FinalitySignal)
/// onto the channel. A chain-layer consumer (see
/// `native-chain/src/bin/launch_api/p2p_events.rs::run_finality_event_loop`)
/// translates that signal into a call to
/// [`Blockchain::set_finalized_height`](wrfcoin_shared_protocol::blockchain::chain::Blockchain::set_finalized_height),
/// which is what actually arms the reorg rejection guard at
/// `plan_reorg()`.
///
/// Without a sender attached, the gadget behaves exactly as before —
/// votes are accumulated and finality is tracked locally, but no signal
/// flows out. This keeps every existing test path and every caller that
/// constructed a gadget with [`BftFinalityGadget::new`] fully
/// backwards compatible.
#[derive(Debug)]
pub struct BftFinalityGadget {
    /// The current validator set, keyed by validator id.
    validators: FxHashMap<String, FinalityValidator>,
    /// Sum of all validator stake weights.
    total_weight: u128,
    /// Votes accumulated per (block_height, block_hash).
    ///
    /// Outer key: block_height. Inner: per-block votes.
    /// A height can have votes for multiple competing blocks (forks).
    votes_by_height: FxHashMap<u64, Vec<BlockVotes>>,
    /// Track which validator voted at which height (to detect equivocation).
    /// Key: (validator_id, block_height) -> block_hash they voted for.
    validator_height_votes: FxHashMap<(String, u64), [u8; 32]>,
    /// The highest finalized block height.
    finalized_height: u64,
    /// Optional channel that receives a [`FinalitySignal`] every time a
    /// block crosses the 2/3+ threshold. See the type-level docs for
    /// wiring details. `None` keeps the gadget backward-compatible.
    finality_sink: Option<FinalitySignalSender>,
    /// The most recently produced [`FinalityCertificate`]. Populated by
    /// [`submit_vote`](Self::submit_vote) whenever a block first crosses
    /// the supermajority threshold. `None` until the first finalization.
    ///
    /// This exists so consumers that want the full vote-set record (for
    /// gossip propagation, audit log, or post-mortem) can retrieve it
    /// right after the `submit_vote` call that produced
    /// `FinalityEvent::BlockFinalized`. The field is overwritten on
    /// every subsequent finalization — callers wanting history should
    /// snapshot on each `BlockFinalized` event.
    last_certificate: Option<FinalityCertificate>,
    /// Epoch (validator-set generation) this gadget serves (#2256). Votes must
    /// name this epoch. Set at construction (default `0`) or via
    /// [`with_epoch`](Self::with_epoch); the gadget's set is fixed for its
    /// lifetime, so a new committee means a new gadget with a new epoch.
    epoch: u64,
    /// Hash of the ordered validator set this gadget serves (#2256), computed
    /// once at construction from the same sorted `(id, stake_weight, public_key)`
    /// projection every node derives, so peers agree. Votes must name this hash.
    validator_set_hash: [u8; 32],
}

/// Deterministic hash of an ordered validator set (#2256).
///
/// Delegates to shared-protocol's [`canonical_validator_set_hash`] — the single
/// definition of a validator set's identity — so this hash is **byte-identical**
/// to `ValidatorSetChange::validator_set_hash` (#2227). A set-change declares the
/// committee that finality votes then bind to; if the two hashes disagreed, a
/// set-change could not name the set the votes reference. The cross-type equality
/// is pinned by `validator_set_hash_agrees_with_validator_set_change_2227_2256`.
pub fn compute_validator_set_hash(validators: &[FinalityValidator]) -> [u8; 32] {
    let hash = wrfcoin_shared_protocol::consensus::validator_set::canonical_validator_set_hash(
        // determinism-ok: this is an ordered slice and canonical_validator_set_hash sorts the projected entries by id before hashing
        validators.iter().map(|v| {
            wrfcoin_shared_protocol::consensus::validator_set::ValidatorSetEntry {
                id: &v.id,
                stake_weight: v.stake_weight,
                public_key: v.public_key,
            }
        }),
    );
    *hash.as_bytes()
}

impl BftFinalityGadget {
    /// Create a new BFT finality gadget with the given validator set.
    ///
    /// # Panics
    ///
    /// Panics if `validators` is empty (a BFT gadget requires at least one
    /// validator).
    pub fn new(validators: Vec<FinalityValidator>) -> Self {
        assert!(!validators.is_empty(), "validator set must not be empty");

        let total_weight: u128 = validators.iter().map(|v| u128::from(v.stake_weight)).sum();
        let validator_set_hash = compute_validator_set_hash(&validators);
        let validator_map: FxHashMap<String, FinalityValidator> =
            validators.into_iter().map(|v| (v.id.clone(), v)).collect();

        Self {
            validators: validator_map,
            total_weight,
            votes_by_height: FxHashMap::default(),
            validator_height_votes: FxHashMap::default(),
            finalized_height: 0,
            finality_sink: None,
            last_certificate: None,
            epoch: 0,
            validator_set_hash,
        }
    }

    /// Builder-style variant that sets the committee [`epoch`](Self::epoch) this
    /// gadget serves (#2256). The `validator_set_hash` is derived from the
    /// validator set regardless; the epoch is the caller's generation counter.
    pub fn with_epoch(mut self, epoch: u64) -> Self {
        self.epoch = epoch;
        self
    }

    /// The epoch (validator-set generation) this gadget serves (#2256).
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The hash of the ordered validator set this gadget serves (#2256). Votes
    /// must carry this exact value (and [`epoch`](Self::epoch)) or they are
    /// rejected as [`BftFinalityError::WrongCommittee`].
    pub fn validator_set_hash(&self) -> [u8; 32] {
        self.validator_set_hash
    }

    /// Ensure a vote names the committee this gadget serves (#2256). Rejects a
    /// vote whose `epoch` or `validator_set_hash` differs from the active set —
    /// the check that stops a vote cast under one committee counting toward a
    /// quorum computed from another's total power.
    fn ensure_committee_scope(&self, vote: &FinalityVote) -> Result<(), BftFinalityError> {
        if vote.epoch == self.epoch && vote.validator_set_hash == self.validator_set_hash {
            return Ok(());
        }
        Err(BftFinalityError::WrongCommittee {
            validator_id: vote.validator_id.clone(),
            vote_epoch: vote.epoch,
            vote_set_hash: hex::encode(vote.validator_set_hash),
            active_epoch: self.epoch,
            active_set_hash: hex::encode(self.validator_set_hash),
        })
    }

    /// Builder-style variant of [`BftFinalityGadget::new`] that pre-installs a
    /// finality sink.
    ///
    /// See the struct-level docs for the end-to-end wiring path that connects
    /// this sink to [`Blockchain::set_finalized_height`](wrfcoin_shared_protocol::blockchain::chain::Blockchain::set_finalized_height).
    pub fn with_finality_sink(
        validators: Vec<FinalityValidator>,
        sink: FinalitySignalSender,
    ) -> Self {
        let mut gadget = Self::new(validators);
        gadget.finality_sink = Some(sink);
        gadget
    }

    /// Attach or replace the finality sink on an existing gadget.
    ///
    /// Useful when the gadget is constructed before the runtime has set up
    /// its channel (e.g. during config parsing) and wired up later during
    /// startup.
    pub fn set_finality_sink(&mut self, sink: FinalitySignalSender) {
        self.finality_sink = Some(sink);
    }

    /// Remove the finality sink, if any. Returns the previous sender so the
    /// caller can decide whether to drop or reuse it.
    pub fn take_finality_sink(&mut self) -> Option<FinalitySignalSender> {
        self.finality_sink.take()
    }

    /// Return the total stake weight of the validator set.
    pub fn total_weight(&self) -> u128 {
        self.total_weight
    }

    /// Return the number of validators in the set.
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    /// Return the highest finalized block height.
    pub fn finalized_height(&self) -> u64 {
        self.finalized_height
    }

    /// Return the most recently produced [`FinalityCertificate`], if any.
    ///
    /// The certificate is the persistable 2/3+1 vote-set that justifies a
    /// block being final. It is populated by
    /// [`submit_vote`](Self::submit_vote) in the same call that first
    /// produces [`FinalityEvent::BlockFinalized`] for a given block, and is
    /// overwritten on every subsequent finalization. Callers wanting a full
    /// history should snapshot on each `BlockFinalized` event — this
    /// accessor only surfaces the most recent one.
    pub fn last_certificate(&self) -> Option<&FinalityCertificate> {
        self.last_certificate.as_ref()
    }

    /// Verify a gossiped [`FinalityCertificate`] without mutating this gadget.
    ///
    /// Certificate gossip is untrusted P2P input, so validation deliberately
    /// replays every certificate vote through the same cryptographic
    /// [`submit_vote_verified`](Self::submit_vote_verified) path used for
    /// individual vote gossip, but against a temporary gadget with no finality
    /// sink attached. That proves validator membership, signatures,
    /// duplicate/equivocation rejection, and 2/3+ quorum before any caller
    /// decides to advance local finality.
    pub fn validate_certificate(&self, cert: &FinalityCertificate) -> Result<(), BftFinalityError> {
        if cert.block_height <= self.finalized_height && self.finalized_height > 0 {
            return Err(BftFinalityError::AlreadyFinalized(cert.block_height));
        }

        if cert.total_validator_weight != self.total_weight {
            return Err(BftFinalityError::InvalidCertificate(format!(
                "reported total validator weight {} does not match local set {}",
                cert.total_validator_weight, self.total_weight
            )));
        }

        // DoS guard: a valid certificate can carry at most one vote per
        // validator. Reject oversized certs before entering the signature
        // verification loop so a malicious peer cannot force O(N) ed25519
        // verifications for an arbitrarily large N.
        let max_votes = self.validators.len();
        if cert.votes.len() > max_votes {
            return Err(BftFinalityError::InvalidCertificate(format!(
                "certificate carries {} votes but validator set has only {} members; \
                 rejecting without signature iteration (DoS guard)",
                cert.votes.len(),
                max_votes
            )));
        }

        // #2256: the replay gadget must serve the *same* committee as `self`,
        // epoch included, so it inherits the committee-scope check — a cert whose
        // votes name a different validator set or epoch is rejected during
        // replay via `submit_vote_verified`.
        let validators: Vec<FinalityValidator> = self.validators.values().cloned().collect();
        let mut replay = BftFinalityGadget::new(validators).with_epoch(self.epoch);
        let mut finalized_weight = None;
        let mut finalized_voter_count = None;

        for vote in &cert.votes {
            if vote.block_height != cert.block_height || vote.block_hash != cert.block_hash {
                return Err(BftFinalityError::InvalidCertificate(format!(
                    "vote from {} targets height {} hash {}, expected height {} hash {}",
                    vote.validator_id,
                    vote.block_height,
                    hex::encode(vote.block_hash),
                    cert.block_height,
                    hex::encode(cert.block_hash)
                )));
            }

            if let FinalityEvent::BlockFinalized {
                total_vote_weight,
                voter_count,
                ..
            } = replay.submit_vote_verified(vote.clone())?
            {
                finalized_weight = Some(total_vote_weight);
                finalized_voter_count = Some(voter_count);
            }
        }

        let Some(total_vote_weight) = finalized_weight else {
            return Err(BftFinalityError::InvalidCertificate(format!(
                "certificate weight {} is below required quorum {}",
                cert.total_vote_weight,
                replay.required_weight()
            )));
        };

        if cert.total_vote_weight != total_vote_weight {
            return Err(BftFinalityError::InvalidCertificate(format!(
                "reported vote weight {} does not match verified weight {}",
                cert.total_vote_weight, total_vote_weight
            )));
        }

        if finalized_voter_count != Some(cert.votes.len()) {
            return Err(BftFinalityError::InvalidCertificate(format!(
                "certificate carries {} votes but finalized after {} verified votes",
                cert.votes.len(),
                finalized_voter_count.unwrap_or_default()
            )));
        }

        Ok(())
    }

    /// Check whether `accumulated` constitutes a 2/3+ supermajority of `total`.
    ///
    /// Uses integer arithmetic: `accumulated * 3 > total * 2`.
    fn is_supermajority(accumulated: u128, total: u128) -> bool {
        accumulated * 3 > total * 2
    }

    /// The minimum stake weight required for finality (ceiling of 2/3 * total + 1).
    pub fn required_weight(&self) -> u128 {
        // We need accumulated * 3 > total * 2, i.e., accumulated > total * 2 / 3.
        // Minimum integer accumulated = floor(total * 2 / 3) + 1.
        (self.total_weight * 2 / 3) + 1
    }

    /// Submit a finality vote.
    ///
    /// Returns:
    /// - `Ok(FinalityEvent::BlockFinalized { .. })` if this vote pushes the
    ///   block past the 2/3+ threshold.
    /// - `Ok(FinalityEvent::VoteReceived { .. })` if the vote was recorded but
    ///   finality is not yet reached.
    /// - `Ok(FinalityEvent::DuplicateVoteRejected { .. })` if the validator
    ///   already voted for the same block at this height.
    /// - `Err(BftFinalityError::UnknownValidator)` if the validator is not in
    ///   the set.
    /// - `Err(BftFinalityError::Equivocation)` if the validator voted for a
    ///   *different* block at the same height.
    /// - `Err(BftFinalityError::AlreadyFinalized)` if the block height is
    ///   already finalized.
    pub fn submit_vote(&mut self, vote: FinalityVote) -> Result<FinalityEvent, BftFinalityError> {
        // 1. Verify the validator is in the set.
        let validator = self
            .validators
            .get(&vote.validator_id)
            .ok_or_else(|| BftFinalityError::UnknownValidator(vote.validator_id.clone()))?
            .clone();

        // 2. Check if this height is already finalized.
        if vote.block_height <= self.finalized_height && self.finalized_height > 0 {
            return Err(BftFinalityError::AlreadyFinalized(vote.block_height));
        }

        // 3. Check for equivocation / duplicate at this height.
        let height_key = (vote.validator_id.clone(), vote.block_height);
        if let Some(prev_hash) = self.validator_height_votes.get(&height_key) {
            if *prev_hash == vote.block_hash {
                // Duplicate vote for the same block -- reject gracefully.
                return Ok(FinalityEvent::DuplicateVoteRejected {
                    block_hash: vote.block_hash,
                    block_height: vote.block_height,
                    validator_id: vote.validator_id,
                });
            } else {
                // Voted for a different block at the same height -- equivocation!
                return Err(BftFinalityError::Equivocation {
                    validator_id: vote.validator_id,
                    block_height: vote.block_height,
                });
            }
        }

        // 4. Record the vote.
        self.validator_height_votes
            .insert(height_key, vote.block_hash);

        let height_entries = self.votes_by_height.entry(vote.block_height).or_default();

        // Find or create a BlockVotes entry for this specific block hash.
        let block_votes = if let Some(bv) = height_entries
            .iter_mut()
            .find(|bv| bv.block_hash == vote.block_hash)
        {
            bv
        } else {
            height_entries.push(BlockVotes::new(vote.block_hash, vote.block_height));
            height_entries.last_mut().unwrap()
        };

        block_votes
            .votes
            .insert(vote.validator_id.clone(), vote.clone());
        block_votes.accumulated_weight += u128::from(validator.stake_weight);

        // 5. Check supermajority.
        if !block_votes.finalized
            && Self::is_supermajority(block_votes.accumulated_weight, self.total_weight)
        {
            block_votes.finalized = true;
            if vote.block_height > self.finalized_height {
                self.finalized_height = vote.block_height;
            }

            let finalized_event = FinalityEvent::BlockFinalized {
                block_hash: vote.block_hash,
                block_height: vote.block_height,
                total_vote_weight: block_votes.accumulated_weight,
                total_validator_weight: self.total_weight,
                voter_count: block_votes.votes.len(),
            };

            // #1388: Build the full-vote-set certificate that justifies this
            // finality. Sorting by validator_id keeps the certificate
            // deterministic across nodes so gossiped bytes are comparable.
            let mut votes_sorted: Vec<FinalityVote> = block_votes.votes.values().cloned().collect();
            votes_sorted.sort_by(|a, b| a.validator_id.cmp(&b.validator_id));
            self.last_certificate = Some(FinalityCertificate {
                block_hash: vote.block_hash,
                block_height: vote.block_height,
                votes: votes_sorted,
                total_vote_weight: block_votes.accumulated_weight,
                total_validator_weight: self.total_weight,
            });

            // #1970: Bound memory now that a height has been finalized. The
            // vote / equivocation maps accumulate one entry per
            // (height, validator) and previously grew without limit because
            // `prune_below` had no production caller — a validator with a valid
            // key could submit votes at arbitrary far-future heights and force
            // unbounded allocation (slow OOM). We prune everything well below
            // the finalized tip (keeping a lag window so recent finalized
            // heights remain available for late-vote equivocation detection and
            // hash lookups). This runs after the certificate is built so the
            // mutable `block_votes` borrow has ended.
            if let Some(watermark) = self.prune_watermark() {
                self.prune_below(watermark);
            }

            // #1216: Forward the event to the chain-layer finality sink so
            // `Blockchain::set_finalized_height` gets called. Without this,
            // `plan_reorg`'s BFT finality guard stays at height 0 forever
            // and every reorg is allowed up to MAX_REORG_DEPTH.
            //
            // `emit_finality_signal` is non-blocking and never panics, so
            // the consensus hot path is unaffected even if the consumer is
            // slow or the channel is closed.
            if let Some(sink) = &self.finality_sink {
                emit_finality_signal(
                    sink,
                    FinalitySignal::new(vote.block_height, vote.block_hash, "bft-gadget"),
                );
            }

            return Ok(finalized_event);
        }

        Ok(FinalityEvent::VoteReceived {
            block_hash: vote.block_hash,
            block_height: vote.block_height,
            validator_id: vote.validator_id,
            vote_count: block_votes.votes.len(),
            accumulated_weight: block_votes.accumulated_weight,
            required_weight: self.required_weight(),
        })
    }

    /// Submit a finality vote, but first cryptographically verify the
    /// signature against the validator's registered public key (#1388).
    ///
    /// This is the entry point used by the p2p ingestion path
    /// (`launch_api/p2p_events.rs`) so that a malicious peer cannot
    /// inject votes attributed to an honest validator. The bare
    /// [`submit_vote`](Self::submit_vote) is kept for tests and trusted
    /// in-process callers (e.g. the local node voting on its own
    /// freshly-validated block) where the signature has already been
    /// produced — in those cases the gadget would just re-verify a
    /// freshly-minted signature it was about to trust anyway.
    ///
    /// Returns:
    /// - `Err(BftFinalityError::UnknownValidator)` if `vote.validator_id`
    ///   is not in the gadget's set.
    /// - `Err(BftFinalityError::NoPublicKey)` if the validator entry
    ///   exists but has no public key registered (the verified path
    ///   refuses to fall through to unauthenticated `submit_vote`).
    /// - `Err(BftFinalityError::MalformedPublicKey)` if the registered
    ///   key bytes are not a valid ed25519 point.
    /// - `Err(BftFinalityError::InvalidSignature)` if the attached
    ///   signature does not verify against the canonical signing
    ///   message [`FinalityVote::content_hash`].
    /// - Otherwise, the same `Ok(FinalityEvent)` /
    ///   `Err(BftFinalityError)` outcomes as [`submit_vote`](Self::submit_vote).
    pub fn submit_vote_verified(
        &mut self,
        vote: FinalityVote,
    ) -> Result<FinalityEvent, BftFinalityError> {
        // #2256: reject a vote scoped to a different committee before doing any
        // signature work. A gossiped vote may carry a perfectly valid signature
        // for the committee it was *actually* cast in; that does not make it
        // countable here. This is the untrusted (peer) ingestion path, which is
        // exactly where a stale-set vote arrives — the trusted in-process
        // `submit_vote` mints votes for its own gadget and needs no such check.
        self.ensure_committee_scope(&vote)?;

        let validator = self
            .validators
            .get(&vote.validator_id)
            .ok_or_else(|| BftFinalityError::UnknownValidator(vote.validator_id.clone()))?;

        let pubkey_bytes = validator
            .public_key
            .ok_or_else(|| BftFinalityError::NoPublicKey(vote.validator_id.clone()))?;

        let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
            .map_err(|_| BftFinalityError::MalformedPublicKey(vote.validator_id.clone()))?;

        let sig = if vote.signature.len() == 64 {
            let mut bytes = [0u8; 64];
            bytes.copy_from_slice(&vote.signature);
            Ed25519Signature::from_bytes(&bytes)
        } else {
            return Err(BftFinalityError::InvalidSignature(
                vote.validator_id.clone(),
            ));
        };

        let signing_message = vote.content_hash();
        verifying_key
            .verify(&signing_message, &sig)
            .map_err(|_| BftFinalityError::InvalidSignature(vote.validator_id.clone()))?;

        self.submit_vote(vote)
    }

    /// Build the canonical bytes a validator must ed25519-sign to
    /// produce a vote that [`submit_vote_verified`](Self::submit_vote_verified)
    /// will accept. Exposed so signers and verifiers stay in lock-step
    /// and integration tests can mint valid votes without duplicating
    /// the hashing logic.
    ///
    /// `epoch` and `validator_set_hash` name the committee the vote is cast into
    /// (#2256); a signer obtains them from the gadget it is voting under via
    /// [`epoch`](Self::epoch) and [`validator_set_hash`](Self::validator_set_hash).
    pub fn signing_message_for(
        block_hash: [u8; 32],
        block_height: u64,
        epoch: u64,
        validator_set_hash: [u8; 32],
        validator_id: &str,
    ) -> [u8; 32] {
        FinalityVote {
            block_hash,
            block_height,
            epoch,
            validator_set_hash,
            validator_id: validator_id.to_string(),
            signature: Vec::new(),
        }
        .content_hash()
    }

    /// Query whether a block at the given height has been finalized.
    pub fn is_finalized(&self, block_height: u64) -> bool {
        if let Some(entries) = self.votes_by_height.get(&block_height) {
            entries.iter().any(|bv| bv.finalized)
        } else {
            false
        }
    }

    /// Get the finalized block hash at a given height, if any.
    pub fn finalized_block_hash(&self, block_height: u64) -> Option<[u8; 32]> {
        self.votes_by_height.get(&block_height).and_then(|entries| {
            entries
                .iter()
                .find(|bv| bv.finalized)
                .map(|bv| bv.block_hash)
        })
    }

    /// Get the number of votes accumulated for a specific block.
    pub fn vote_count(&self, block_height: u64, block_hash: &[u8; 32]) -> usize {
        self.votes_by_height
            .get(&block_height)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|bv| &bv.block_hash == block_hash)
                    .map(|bv| bv.votes.len())
            })
            .unwrap_or(0)
    }

    /// Get the accumulated stake weight for a specific block.
    pub fn accumulated_weight(&self, block_height: u64, block_hash: &[u8; 32]) -> u128 {
        self.votes_by_height
            .get(&block_height)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|bv| &bv.block_hash == block_hash)
                    .map(|bv| bv.accumulated_weight)
            })
            .unwrap_or(0)
    }

    /// Compute the safe auto-prune watermark relative to the current finalized
    /// height (#1970).
    ///
    /// Returns `Some(height)` such that vote/equivocation data for heights at or
    /// below `height` can be dropped, or `None` when the finalized tip has not
    /// yet advanced far enough to prune anything (i.e. the lag window still
    /// covers all observed heights).
    ///
    /// The watermark is `finalized_height - lag`, where
    /// `lag = max(MIN_PRUNE_LAG, validator_count * PRUNE_LAG_MULTIPLIER)`.
    /// Because pruning is gated on a height having *been finalized*, and the
    /// retained window only ever covers already-finalized history plus the
    /// in-flight tip, this never removes data still needed for liveness or
    /// safety: `validate_certificate` already rejects any certificate at or
    /// below `finalized_height`, and equivocation for heights inside the lag
    /// window remains detectable.
    fn prune_watermark(&self) -> Option<u64> {
        let lag = (self.validators.len() as u64)
            .saturating_mul(PRUNE_LAG_MULTIPLIER)
            .max(MIN_PRUNE_LAG);
        // `finalized_height - lag - 1` is the highest height that is strictly
        // below the retained window. Saturating subtraction yields 0 when the
        // tip is still within `lag` of genesis; in that case there is nothing
        // safe to prune yet, so return `None`.
        let cutoff = self.finalized_height.saturating_sub(lag);
        if cutoff == 0 {
            None
        } else {
            // Prune strictly below the window: heights <= cutoff - 1.
            Some(cutoff - 1)
        }
    }

    /// Prune vote data for heights at or below the given threshold.
    ///
    /// This is useful for memory management in long-running nodes. Only prune
    /// heights that have already been finalized.
    ///
    /// Wired into the production finality path via [`Self::submit_vote`] (#1970);
    /// also callable directly by operators / tests.
    pub fn prune_below(&mut self, height: u64) {
        // #2028: never prune data for a height that has not yet been finalized.
        // `prune_below` is public and the vote / equivocation maps are
        // consensus-critical: `validator_height_votes` is the *only* thing that
        // catches a same-height double-vote (the persistent `EvidenceStore` is
        // not consulted by `submit_vote`), so erasing it for an
        // unfinalized height opens an equivocation-detection blind spot — a
        // validator could then vote for a different block at that height with no
        // `Equivocation` error, because the `AlreadyFinalized` guard only covers
        // heights `<= finalized_height`. Erasing `votes_by_height` for an
        // unfinalized height would likewise discard in-flight vote
        // accumulation. Clamp the prune ceiling to the finalized tip so only
        // already-finalized history (already protected by `AlreadyFinalized`)
        // is ever dropped. The production auto-prune caller (#1970) passes a
        // watermark strictly below `finalized_height`, so this clamp is a no-op
        // on that path; it only constrains direct operator/test calls.
        let ceiling = height.min(self.finalized_height);

        let heights_to_remove: Vec<u64> = self
            .votes_by_height
            .keys()
            .copied()
            .filter(|h| *h <= ceiling)
            .collect();

        for h in heights_to_remove {
            self.votes_by_height.remove(&h);
        }

        // Also clean up validator_height_votes for pruned (finalized) heights.
        self.validator_height_votes
            .retain(|(_vid, h), _| *h > ceiling);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// #2249: the cross-crate `DomainSeparated` impl uses the tag reserved in the
    /// shared-protocol registry, and its preimage emits that tag first (so a vote
    /// signature cannot be reinterpreted as another signed object) while excluding
    /// the signature field.
    #[test]
    fn finality_vote_is_domain_separated_2249() {
        use wrfcoin_shared_protocol::crypto::domain_registry::{
            tag_matches_registry, SignedObjectKind,
        };
        use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;

        assert!(tag_matches_registry::<FinalityVote>(
            SignedObjectKind::FinalityVote
        ));

        let vote = FinalityVote {
            block_hash: [7u8; 32],
            block_height: 42,
            epoch: 0,
            validator_set_hash: [0u8; 32],
            validator_id: "validator-1".to_string(),
            signature: vec![1, 2, 3],
        };
        let preimage = vote.signing_preimage();
        let tag = FinalityVote::DOMAIN_TAG;
        assert_eq!(&preimage[0..8], &(tag.len() as u64).to_le_bytes());
        assert_eq!(&preimage[8..8 + tag.len()], tag);

        // #2249 sensitivity: the signature is excluded (mutating it leaves the
        // preimage identical — RED if `signature` is folded into canonical_body),
        // and a signed field is bound (mutating block_height moves the preimage —
        // RED if `.u64_field(self.block_height)` is removed).
        let mut sig_only = vote.clone();
        sig_only.signature = vec![9, 9, 9, 9, 9];
        assert_eq!(
            preimage,
            sig_only.signing_preimage(),
            "signature must be excluded from the domain preimage"
        );
        let mut changed = vote.clone();
        changed.block_height = 43;
        assert_ne!(
            preimage,
            changed.signing_preimage(),
            "a signed field (block_height) must be bound into the domain preimage"
        );
    }

    /// Helper: create a validator set with `n` validators, each having `stake` weight.
    fn make_validators(n: usize, stake: u64) -> Vec<FinalityValidator> {
        (0..n)
            .map(|i| FinalityValidator {
                id: format!("validator-{}", i),
                stake_weight: stake,
                public_key: None,
            })
            .collect()
    }

    /// Helper: create a vote for a given block from a given validator.
    ///
    /// Trusted-path (`submit_vote`) helper: `epoch`/`validator_set_hash` are left
    /// at their zero defaults because `submit_vote` does not enforce committee
    /// scope (#2256 — that check lives on the untrusted `submit_vote_verified`
    /// path, exercised via [`signed_vote`]).
    fn make_vote(validator_id: &str, block_height: u64, block_hash: [u8; 32]) -> FinalityVote {
        FinalityVote {
            block_hash,
            block_height,
            epoch: 0,
            validator_set_hash: [0u8; 32],
            validator_id: validator_id.to_string(),
            signature: vec![0u8; 64], // placeholder signature
        }
    }

    /// Helper: create a deterministic block hash from a seed.
    fn block_hash(seed: u8) -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0] = seed;
        h
    }

    // -----------------------------------------------------------------------
    // Test 1: Vote accumulation -- votes are counted correctly
    // -----------------------------------------------------------------------
    #[test]
    fn test_vote_accumulation() {
        let validators = make_validators(4, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(1);

        // Submit first vote
        let event = gadget
            .submit_vote(make_vote("validator-0", 1, hash))
            .unwrap();
        match event {
            FinalityEvent::VoteReceived {
                vote_count,
                accumulated_weight,
                ..
            } => {
                assert_eq!(vote_count, 1);
                assert_eq!(accumulated_weight, 100);
            },
            other => panic!("expected VoteReceived, got {:?}", other),
        }

        // Submit second vote
        let event = gadget
            .submit_vote(make_vote("validator-1", 1, hash))
            .unwrap();
        match event {
            FinalityEvent::VoteReceived {
                vote_count,
                accumulated_weight,
                ..
            } => {
                assert_eq!(vote_count, 2);
                assert_eq!(accumulated_weight, 200);
            },
            other => panic!("expected VoteReceived, got {:?}", other),
        }

        assert_eq!(gadget.vote_count(1, &hash), 2);
        assert_eq!(gadget.accumulated_weight(1, &hash), 200);
    }

    // -----------------------------------------------------------------------
    // Test 2: Just below 2/3 threshold -- no finality
    // -----------------------------------------------------------------------
    #[test]
    fn test_just_below_threshold_no_finality() {
        // 3 validators, each with weight 100 => total = 300.
        // 2/3 of 300 = 200. Need strictly > 200, so 200 is NOT enough.
        let validators = make_validators(3, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(1);

        // Submit 2 of 3 votes (200 / 300 = exactly 2/3).
        let event1 = gadget
            .submit_vote(make_vote("validator-0", 1, hash))
            .unwrap();
        assert!(matches!(event1, FinalityEvent::VoteReceived { .. }));

        let event2 = gadget
            .submit_vote(make_vote("validator-1", 1, hash))
            .unwrap();
        // 200 * 3 = 600, 300 * 2 = 600 -> NOT strictly greater, so no finality.
        assert!(
            matches!(event2, FinalityEvent::VoteReceived { .. }),
            "exactly 2/3 should NOT trigger finality"
        );
        assert!(!gadget.is_finalized(1));
    }

    // -----------------------------------------------------------------------
    // Test 3: At the 2/3+ threshold -- finality declared
    // -----------------------------------------------------------------------
    #[test]
    fn test_at_supermajority_threshold_finality() {
        // 3 validators, each with weight 100 => total = 300.
        // Need > 200. Third vote brings to 300 > 200.
        let validators = make_validators(3, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(1);

        gadget
            .submit_vote(make_vote("validator-0", 1, hash))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-1", 1, hash))
            .unwrap();

        let event = gadget
            .submit_vote(make_vote("validator-2", 1, hash))
            .unwrap();
        match event {
            FinalityEvent::BlockFinalized {
                block_height,
                total_vote_weight,
                total_validator_weight,
                voter_count,
                ..
            } => {
                assert_eq!(block_height, 1);
                assert_eq!(total_vote_weight, 300);
                assert_eq!(total_validator_weight, 300);
                assert_eq!(voter_count, 3);
            },
            other => panic!("expected BlockFinalized, got {:?}", other),
        }
        assert!(gadget.is_finalized(1));
        assert_eq!(gadget.finalized_height(), 1);
    }

    // -----------------------------------------------------------------------
    // Test 4: Above 2/3 threshold -- finality also declared (weighted)
    // -----------------------------------------------------------------------
    #[test]
    fn test_above_threshold_weighted_validators() {
        // 4 validators with unequal stakes: [100, 100, 100, 200] => total = 500.
        // Need > 333 (i.e., >= 334). The heavy validator alone is only 200.
        // Heavy + one light = 300, still <= 333.
        // Heavy + two lights = 400 > 333 => finality.
        let validators = vec![
            FinalityValidator {
                id: "v-0".into(),
                stake_weight: 100,
                public_key: None,
            },
            FinalityValidator {
                id: "v-1".into(),
                stake_weight: 100,
                public_key: None,
            },
            FinalityValidator {
                id: "v-2".into(),
                stake_weight: 100,
                public_key: None,
            },
            FinalityValidator {
                id: "v-heavy".into(),
                stake_weight: 200,
                public_key: None,
            },
        ];
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(42);

        // Heavy validator votes first (200 / 500).
        let e1 = gadget.submit_vote(make_vote("v-heavy", 5, hash)).unwrap();
        assert!(matches!(e1, FinalityEvent::VoteReceived { .. }));

        // Add one light validator (300 / 500) -- still below.
        let e2 = gadget.submit_vote(make_vote("v-0", 5, hash)).unwrap();
        assert!(
            matches!(e2, FinalityEvent::VoteReceived { .. }),
            "300/500 = 0.6 < 2/3"
        );

        // Add second light validator (400 / 500) -- finality!
        let e3 = gadget.submit_vote(make_vote("v-1", 5, hash)).unwrap();
        assert!(
            matches!(e3, FinalityEvent::BlockFinalized { .. }),
            "400/500 = 0.8 > 2/3"
        );
    }

    // -----------------------------------------------------------------------
    // Test 5: Duplicate vote is rejected gracefully
    // -----------------------------------------------------------------------
    #[test]
    fn test_duplicate_vote_rejected() {
        let validators = make_validators(4, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(1);

        gadget
            .submit_vote(make_vote("validator-0", 1, hash))
            .unwrap();

        // Same validator, same block -> duplicate
        let event = gadget
            .submit_vote(make_vote("validator-0", 1, hash))
            .unwrap();
        assert!(
            matches!(
                &event,
                FinalityEvent::DuplicateVoteRejected {
                    validator_id,
                    ..
                } if validator_id == "validator-0"
            ),
            "duplicate vote should be rejected, got {:?}",
            event
        );

        // Duplicate should NOT increase vote count or weight.
        assert_eq!(gadget.vote_count(1, &hash), 1);
        assert_eq!(gadget.accumulated_weight(1, &hash), 100);
    }

    // -----------------------------------------------------------------------
    // Test 6: Equivocation detected -- validator votes for different block
    // -----------------------------------------------------------------------
    #[test]
    fn test_equivocation_detected() {
        let validators = make_validators(4, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash_a = block_hash(1);
        let hash_b = block_hash(2);

        gadget
            .submit_vote(make_vote("validator-0", 1, hash_a))
            .unwrap();

        // Same validator, same height, different block hash -> equivocation.
        let result = gadget.submit_vote(make_vote("validator-0", 1, hash_b));
        assert!(
            matches!(
                result,
                Err(BftFinalityError::Equivocation {
                    ref validator_id,
                    block_height: 1,
                }) if validator_id == "validator-0"
            ),
            "expected equivocation error, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // Test 7: Unknown validator is rejected
    // -----------------------------------------------------------------------
    #[test]
    fn test_unknown_validator_rejected() {
        let validators = make_validators(3, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(1);

        let result = gadget.submit_vote(make_vote("unknown-validator", 1, hash));
        assert!(
            matches!(result, Err(BftFinalityError::UnknownValidator(ref id)) if id == "unknown-validator"),
            "expected UnknownValidator error, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // Test 8: Already-finalized height is rejected
    // -----------------------------------------------------------------------
    #[test]
    fn test_already_finalized_rejected() {
        // 3 validators -> need all 3 for finality (since 2/3 exactly is not enough).
        let validators = make_validators(3, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(1);

        // Finalize height 5.
        gadget
            .submit_vote(make_vote("validator-0", 5, hash))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-1", 5, hash))
            .unwrap();
        let event = gadget
            .submit_vote(make_vote("validator-2", 5, hash))
            .unwrap();
        assert!(matches!(event, FinalityEvent::BlockFinalized { .. }));
        assert_eq!(gadget.finalized_height(), 5);

        // Try to vote on height 3 (already finalized since <= 5).
        let result = gadget.submit_vote(make_vote("validator-0", 3, hash));
        assert!(
            matches!(result, Err(BftFinalityError::AlreadyFinalized(3))),
            "expected AlreadyFinalized(3), got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // Test 9: Finality event contains correct block hash
    // -----------------------------------------------------------------------
    #[test]
    fn test_finality_event_block_hash() {
        let validators = make_validators(3, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(99);

        gadget
            .submit_vote(make_vote("validator-0", 10, hash))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-1", 10, hash))
            .unwrap();
        let event = gadget
            .submit_vote(make_vote("validator-2", 10, hash))
            .unwrap();

        match event {
            FinalityEvent::BlockFinalized {
                block_hash: finalized_hash,
                block_height,
                ..
            } => {
                assert_eq!(finalized_hash, hash);
                assert_eq!(block_height, 10);
            },
            other => panic!("expected BlockFinalized, got {:?}", other),
        }

        assert_eq!(gadget.finalized_block_hash(10), Some(hash));
    }

    // -----------------------------------------------------------------------
    // Test 10: Pruning removes old vote data
    // -----------------------------------------------------------------------
    #[test]
    fn test_prune_below() {
        let validators = make_validators(3, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(1);

        // Finalize heights 1, 2, 3 (all 3 validators each — exactly 2/3 is not
        // enough). #2028: `prune_below` now clamps its ceiling to
        // `finalized_height`, so the heights must actually be finalized for the
        // prune to remove them. (`MIN_PRUNE_LAG == 64` keeps the #1970 auto-prune
        // dormant at these low heights, so the explicit call below does the work.)
        for height in 1..=3 {
            for v in 0..3 {
                gadget
                    .submit_vote(make_vote(&format!("validator-{v}"), height, hash))
                    .unwrap();
            }
        }
        assert_eq!(gadget.finalized_height(), 3);

        assert_eq!(gadget.vote_count(1, &hash), 3);
        assert_eq!(gadget.vote_count(2, &hash), 3);
        assert_eq!(gadget.vote_count(3, &hash), 3);

        // Prune heights <= 2 (all finalized, so the clamp is a no-op here).
        gadget.prune_below(2);

        assert_eq!(gadget.vote_count(1, &hash), 0);
        assert_eq!(gadget.vote_count(2, &hash), 0);
        assert_eq!(gadget.vote_count(3, &hash), 3); // preserved
    }

    // -----------------------------------------------------------------------
    // Test 10c (#2028): `prune_below` must not erase the per-height vote record
    // for a height that has NOT been finalized. Before the fix, calling
    // `prune_below(h)` with `h > finalized_height` wiped `validator_height_votes`
    // for the unfinalized height, so a validator could then double-vote there
    // for a different block with no `Equivocation` error (the `AlreadyFinalized`
    // guard only covers heights <= finalized_height). The clamp to
    // `finalized_height` closes that blind spot.
    // -----------------------------------------------------------------------
    #[test]
    fn test_prune_below_preserves_unfinalized_equivocation_record_2028() {
        let validators = make_validators(4, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash_a = block_hash(1);
        let hash_b = block_hash(2);

        // A single validator votes at height 7. With 4 validators the 2/3
        // threshold is not met, so height 7 is NOT finalized.
        gadget
            .submit_vote(make_vote("validator-0", 7, hash_a))
            .unwrap();
        assert_eq!(gadget.finalized_height(), 0, "height 7 must be unfinalized");

        // Operator/test prunes below a height above the (zero) finalized tip.
        // The clamp means nothing at height 7 is actually removed.
        gadget.prune_below(100);

        // The same validator now votes for a *different* block at height 7.
        // This must still be caught as equivocation — the record survived.
        let result = gadget.submit_vote(make_vote("validator-0", 7, hash_b));
        assert!(
            matches!(
                result,
                Err(BftFinalityError::Equivocation {
                    ref validator_id,
                    block_height: 7,
                }) if validator_id == "validator-0"
            ),
            "equivocation at an unfinalized height must survive prune_below, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // Test 10b (#1970): auto-prune on finalization bounds map growth.
    //
    // Before #1970, `prune_below` had no production caller, so a validator
    // submitting votes across an ever-increasing height range grew
    // `votes_by_height` and `validator_height_votes` without limit. We now
    // prune below a safe watermark every time a height finalizes. Finalize a
    // long run of heights and assert the internal maps stay bounded by the lag
    // window (O(validator_count)) rather than O(num_heights_finalized).
    // -----------------------------------------------------------------------
    #[test]
    fn test_auto_prune_bounds_vote_maps_on_finalization() {
        // Distinct 32-byte hash per height (u64-wide; the `block_hash(u8)`
        // helper would alias once heights exceed 255).
        let hash_for = |h: u64| -> [u8; 32] {
            let mut out = [0u8; 32];
            out[..8].copy_from_slice(&h.to_be_bytes());
            out
        };
        let validator_count = 3;
        let validators = make_validators(validator_count, 100);
        let mut gadget = BftFinalityGadget::new(validators);

        // Lag window the production path retains (mirrors prune_watermark()).
        let lag = ((validator_count as u64) * PRUNE_LAG_MULTIPLIER).max(MIN_PRUNE_LAG);

        // Finalize many more heights than the lag window so pruning must engage.
        let total_heights: u64 = lag * 10 + 50;
        for height in 1..=total_heights {
            let hash = hash_for(height);
            for v in 0..validator_count {
                gadget
                    .submit_vote(make_vote(&format!("validator-{v}"), height, hash))
                    .unwrap();
            }
        }

        assert_eq!(gadget.finalized_height(), total_heights);

        // The maps must be bounded by the retention window, NOT by total_heights.
        // Allow a small slack above `lag` for the in-flight tip and off-by-one.
        let max_retained = (lag + 2) as usize;
        assert!(
            gadget.votes_by_height.len() <= max_retained,
            "votes_by_height grew unbounded: {} entries (expected <= {})",
            gadget.votes_by_height.len(),
            max_retained,
        );
        assert!(
            gadget.validator_height_votes.len() <= max_retained * validator_count,
            "validator_height_votes grew unbounded: {} entries (expected <= {})",
            gadget.validator_height_votes.len(),
            max_retained * validator_count,
        );

        // Sanity: an early height is gone, the tip is retained.
        assert_eq!(gadget.vote_count(1, &hash_for(1)), 0, "early height pruned");
        assert_eq!(
            gadget.vote_count(total_heights, &hash_for(total_heights)),
            validator_count,
            "tip height retained",
        );
    }

    // -----------------------------------------------------------------------
    // Test 10c (#1970): prune watermark keeps a recent finalized history
    // window intact so late-arriving votes inside the window are still
    // attributable (equivocation detection) and not silently dropped.
    // -----------------------------------------------------------------------
    #[test]
    fn test_auto_prune_retains_recent_window() {
        let hash_for = |h: u64| -> [u8; 32] {
            let mut out = [0u8; 32];
            out[..8].copy_from_slice(&h.to_be_bytes());
            out
        };
        let validator_count = 3;
        let validators = make_validators(validator_count, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let lag = ((validator_count as u64) * PRUNE_LAG_MULTIPLIER).max(MIN_PRUNE_LAG);

        let total_heights: u64 = lag + 30;
        for height in 1..=total_heights {
            let hash = hash_for(height);
            for v in 0..validator_count {
                gadget
                    .submit_vote(make_vote(&format!("validator-{v}"), height, hash))
                    .unwrap();
            }
        }

        // Heights within `lag` of the tip must still be present.
        let inside_window = total_heights - 1; // one below the tip
        assert_eq!(
            gadget.vote_count(inside_window, &hash_for(inside_window)),
            validator_count,
            "height inside the lag window must be retained",
        );
    }

    // -----------------------------------------------------------------------
    // Test 11: Multiple blocks at the same height (fork scenario)
    // -----------------------------------------------------------------------
    #[test]
    fn test_competing_blocks_at_same_height() {
        // 4 validators, each weight 100. Need > 266 for finality.
        let validators = make_validators(4, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash_a = block_hash(1);
        let hash_b = block_hash(2);

        // 2 validators vote for block A.
        gadget
            .submit_vote(make_vote("validator-0", 1, hash_a))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-1", 1, hash_a))
            .unwrap();

        // 2 validators vote for block B.
        gadget
            .submit_vote(make_vote("validator-2", 1, hash_b))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-3", 1, hash_b))
            .unwrap();

        // Neither block should be finalized (200/400 = exactly 1/2 < 2/3).
        assert!(!gadget.is_finalized(1));
        assert_eq!(gadget.vote_count(1, &hash_a), 2);
        assert_eq!(gadget.vote_count(1, &hash_b), 2);
    }

    // -----------------------------------------------------------------------
    // Test: BFT gadget emits a FinalitySignal on the attached channel
    // when a block crosses the 2/3+ threshold (#1216).
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn finality_signal_is_emitted_when_block_is_finalized() {
        use wrfcoin_shared_protocol::consensus::finality::finality_signal_channel;

        // 3 validators each with weight 100 => need all three for finality.
        let validators = make_validators(3, 100);
        let (tx, mut rx) = finality_signal_channel();
        let mut gadget = BftFinalityGadget::with_finality_sink(validators, tx);
        let hash = block_hash(77);

        // Two votes: no signal yet (below threshold).
        gadget
            .submit_vote(make_vote("validator-0", 12, hash))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-1", 12, hash))
            .unwrap();
        assert!(rx.try_recv().is_err(), "no signal before supermajority");

        // Third vote pushes over the threshold — the gadget MUST emit.
        let event = gadget
            .submit_vote(make_vote("validator-2", 12, hash))
            .unwrap();
        assert!(matches!(event, FinalityEvent::BlockFinalized { .. }));

        let signal = rx.recv().await.expect("a finality signal must be emitted");
        assert_eq!(signal.height, 12);
        assert_eq!(signal.block_hash, hash);
        assert_eq!(signal.source, "bft-gadget");
    }

    // -----------------------------------------------------------------------
    // Test: The gadget without a finality sink behaves exactly as before
    // (backwards-compatibility for every existing caller).
    // -----------------------------------------------------------------------
    #[test]
    fn gadget_without_sink_still_reaches_finality() {
        let validators = make_validators(3, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        assert!(gadget.finality_sink.is_none());

        let hash = block_hash(7);
        gadget
            .submit_vote(make_vote("validator-0", 1, hash))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-1", 1, hash))
            .unwrap();
        let event = gadget
            .submit_vote(make_vote("validator-2", 1, hash))
            .unwrap();
        assert!(matches!(event, FinalityEvent::BlockFinalized { .. }));
        assert!(gadget.is_finalized(1));
    }

    // -----------------------------------------------------------------------
    // Test: set_finality_sink can be called after construction.
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn set_finality_sink_works_after_construction() {
        use wrfcoin_shared_protocol::consensus::finality::finality_signal_channel;

        let validators = make_validators(3, 100);
        let mut gadget = BftFinalityGadget::new(validators);

        let (tx, mut rx) = finality_signal_channel();
        gadget.set_finality_sink(tx);

        let hash = block_hash(3);
        gadget
            .submit_vote(make_vote("validator-0", 2, hash))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-1", 2, hash))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-2", 2, hash))
            .unwrap();

        let signal = rx.recv().await.expect("signal must arrive");
        assert_eq!(signal.height, 2);
    }

    // -----------------------------------------------------------------------
    // Test 12: required_weight and is_supermajority edge cases
    // -----------------------------------------------------------------------
    #[test]
    fn test_supermajority_integer_arithmetic() {
        // total = 10. 2/3 of 10 = 6.666... Need > 6.666, so 7 suffices.
        // required_weight = floor(10 * 2 / 3) + 1 = 6 + 1 = 7.
        let validators = vec![
            FinalityValidator {
                id: "a".into(),
                stake_weight: 3,
                public_key: None,
            },
            FinalityValidator {
                id: "b".into(),
                stake_weight: 3,
                public_key: None,
            },
            FinalityValidator {
                id: "c".into(),
                stake_weight: 4,
                public_key: None,
            },
        ];
        let gadget = BftFinalityGadget::new(validators);
        assert_eq!(gadget.total_weight(), 10);
        assert_eq!(gadget.required_weight(), 7);

        // 6 * 3 = 18, 10 * 2 = 20 -> 18 > 20? No.
        assert!(!BftFinalityGadget::is_supermajority(6, 10));
        // 7 * 3 = 21 > 20? Yes.
        assert!(BftFinalityGadget::is_supermajority(7, 10));
    }

    // -----------------------------------------------------------------------
    // Test (#1388): FinalityCertificate is populated on supermajority.
    //
    // Acceptance test from the round-1 handoff:
    //   "with 2/3+1 honest-vote input, certificate is produced; below
    //    threshold, None."
    // -----------------------------------------------------------------------
    #[test]
    fn certificate_is_produced_when_supermajority_reached() {
        let validators = make_validators(4, 100);
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(9);

        // Below threshold: 2 of 4 votes = 200/400 = 50% — not a supermajority.
        gadget
            .submit_vote(make_vote("validator-0", 1, hash))
            .unwrap();
        gadget
            .submit_vote(make_vote("validator-1", 1, hash))
            .unwrap();
        assert!(
            gadget.last_certificate().is_none(),
            "no certificate before supermajority — 50% of stake should not finalize"
        );

        // Cross the threshold: 3 of 4 votes = 300/400 = 75% > 2/3.
        let event = gadget
            .submit_vote(make_vote("validator-2", 1, hash))
            .unwrap();
        assert!(matches!(event, FinalityEvent::BlockFinalized { .. }));

        let cert = gadget
            .last_certificate()
            .expect("certificate must be produced on supermajority");
        assert_eq!(cert.block_height, 1);
        assert_eq!(cert.block_hash, hash);
        assert_eq!(cert.total_validator_weight, 400);
        assert_eq!(
            cert.total_vote_weight, 300,
            "certificate weight must match the accumulated voted stake"
        );
        assert_eq!(
            cert.votes.len(),
            3,
            "certificate must carry exactly the votes that produced finality"
        );
        // Deterministic ordering: votes sorted by validator_id.
        let ids: Vec<&str> = cert.votes.iter().map(|v| v.validator_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["validator-0", "validator-1", "validator-2"],
            "certificate votes must be sorted by validator_id for cross-node determinism"
        );
    }

    // -----------------------------------------------------------------------
    // Test (#1388): below the supermajority threshold, last_certificate
    // stays None — even when many votes accumulate but never cross 2/3+1.
    // -----------------------------------------------------------------------
    // -----------------------------------------------------------------------
    // Test (#1388): submit_vote_verified happy + sad paths.
    // -----------------------------------------------------------------------

    fn signing_keypair_for(seed: u8) -> ed25519_dalek::SigningKey {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        ed25519_dalek::SigningKey::from_bytes(&bytes)
    }

    /// Mint a committee-scoped, signed vote for `gadget` (#2256): the vote
    /// carries the gadget's `epoch` and `validator_set_hash`, and the signature
    /// covers them, so `submit_vote_verified` accepts it as in-committee.
    fn signed_vote(
        gadget: &BftFinalityGadget,
        sk: &ed25519_dalek::SigningKey,
        validator_id: &str,
        block_height: u64,
        block_hash: [u8; 32],
    ) -> FinalityVote {
        use ed25519_dalek::Signer;
        let epoch = gadget.epoch();
        let validator_set_hash = gadget.validator_set_hash();
        let msg = BftFinalityGadget::signing_message_for(
            block_hash,
            block_height,
            epoch,
            validator_set_hash,
            validator_id,
        );
        let sig = sk.sign(&msg);
        FinalityVote {
            block_hash,
            block_height,
            epoch,
            validator_set_hash,
            validator_id: validator_id.to_string(),
            signature: sig.to_bytes().to_vec(),
        }
    }

    #[test]
    fn submit_vote_verified_accepts_valid_signature() {
        let sk0 = signing_keypair_for(1);
        let sk1 = signing_keypair_for(2);
        let sk2 = signing_keypair_for(3);
        let validators = vec![
            FinalityValidator {
                id: "v0".into(),
                stake_weight: 100,
                public_key: Some(sk0.verifying_key().to_bytes()),
            },
            FinalityValidator {
                id: "v1".into(),
                stake_weight: 100,
                public_key: Some(sk1.verifying_key().to_bytes()),
            },
            FinalityValidator {
                id: "v2".into(),
                stake_weight: 100,
                public_key: Some(sk2.verifying_key().to_bytes()),
            },
        ];
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(42);
        gadget
            .submit_vote_verified(signed_vote(&gadget, &sk0, "v0", 7, hash))
            .unwrap();
        gadget
            .submit_vote_verified(signed_vote(&gadget, &sk1, "v1", 7, hash))
            .unwrap();
        let event = gadget
            .submit_vote_verified(signed_vote(&gadget, &sk2, "v2", 7, hash))
            .unwrap();
        assert!(matches!(event, FinalityEvent::BlockFinalized { .. }));
        assert!(gadget.is_finalized(7));
    }

    #[test]
    fn validate_certificate_replays_signatures_and_quorum_without_mutating() {
        let sk0 = signing_keypair_for(1);
        let sk1 = signing_keypair_for(2);
        let sk2 = signing_keypair_for(3);
        let validators = vec![
            FinalityValidator {
                id: "v0".into(),
                stake_weight: 100,
                public_key: Some(sk0.verifying_key().to_bytes()),
            },
            FinalityValidator {
                id: "v1".into(),
                stake_weight: 100,
                public_key: Some(sk1.verifying_key().to_bytes()),
            },
            FinalityValidator {
                id: "v2".into(),
                stake_weight: 100,
                public_key: Some(sk2.verifying_key().to_bytes()),
            },
        ];
        let gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(43);
        let cert = FinalityCertificate {
            block_hash: hash,
            block_height: 9,
            votes: vec![
                signed_vote(&gadget, &sk0, "v0", 9, hash),
                signed_vote(&gadget, &sk1, "v1", 9, hash),
                signed_vote(&gadget, &sk2, "v2", 9, hash),
            ],
            total_vote_weight: 300,
            total_validator_weight: 300,
        };

        gadget
            .validate_certificate(&cert)
            .expect("valid certificate must verify");
        assert_eq!(
            gadget.finalized_height(),
            0,
            "read-only certificate validation must not mutate gadget finality"
        );
    }

    #[test]
    fn validate_certificate_rejects_forged_signature_and_below_quorum() {
        let sk0 = signing_keypair_for(1);
        let sk1 = signing_keypair_for(2);
        let sk2 = signing_keypair_for(3);
        let attacker = signing_keypair_for(99);
        let validators = vec![
            FinalityValidator {
                id: "v0".into(),
                stake_weight: 100,
                public_key: Some(sk0.verifying_key().to_bytes()),
            },
            FinalityValidator {
                id: "v1".into(),
                stake_weight: 100,
                public_key: Some(sk1.verifying_key().to_bytes()),
            },
            FinalityValidator {
                id: "v2".into(),
                stake_weight: 100,
                public_key: Some(sk2.verifying_key().to_bytes()),
            },
        ];
        let gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(44);

        let forged = FinalityCertificate {
            block_hash: hash,
            block_height: 10,
            votes: vec![
                signed_vote(&gadget, &sk0, "v0", 10, hash),
                signed_vote(&gadget, &sk1, "v1", 10, hash),
                signed_vote(&gadget, &attacker, "v2", 10, hash),
            ],
            total_vote_weight: 300,
            total_validator_weight: 300,
        };
        let forged_err = gadget.validate_certificate(&forged).unwrap_err();
        assert!(
            matches!(forged_err, BftFinalityError::InvalidSignature(ref id) if id == "v2"),
            "forged certificate vote must be rejected by signature validation, got {:?}",
            forged_err
        );

        let below_quorum = FinalityCertificate {
            block_hash: hash,
            block_height: 11,
            votes: vec![
                signed_vote(&gadget, &sk0, "v0", 11, hash),
                signed_vote(&gadget, &sk1, "v1", 11, hash),
            ],
            total_vote_weight: 200,
            total_validator_weight: 300,
        };
        let quorum_err = gadget.validate_certificate(&below_quorum).unwrap_err();
        assert!(
            matches!(quorum_err, BftFinalityError::InvalidCertificate(ref reason) if reason.contains("below required quorum")),
            "below-quorum certificate must be rejected, got {:?}",
            quorum_err
        );
    }

    /// DoS guard: validate_certificate must reject a cert that carries more
    /// votes than there are validators in the set, without entering the
    /// signature verification loop.  A malicious peer could otherwise send
    /// an arbitrarily large `cert.votes` vector to trigger O(N) expensive
    /// ed25519 verifications.
    #[test]
    fn validate_certificate_rejects_oversized_votes_vector_before_sig_loop() {
        let sk0 = signing_keypair_for(1);
        let sk1 = signing_keypair_for(2);
        let validators = vec![
            FinalityValidator {
                id: "v0".into(),
                stake_weight: 100,
                public_key: Some(sk0.verifying_key().to_bytes()),
            },
            FinalityValidator {
                id: "v1".into(),
                stake_weight: 100,
                public_key: Some(sk1.verifying_key().to_bytes()),
            },
        ];
        let gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(77);

        // Build a cert with 3 votes (> 2 validators). The extra vote is for a
        // nonexistent validator to ensure the DoS guard fires first — not the
        // unknown-validator error inside the loop.
        let oversized = FinalityCertificate {
            block_hash: hash,
            block_height: 1,
            votes: vec![
                signed_vote(&gadget, &sk0, "v0", 1, hash),
                signed_vote(&gadget, &sk1, "v1", 1, hash),
                // Third vote: would be an unknown-validator but the DoS guard
                // must fire first, before any iteration.
                signed_vote(&gadget, &sk0, "v0", 1, hash),
            ],
            total_vote_weight: 300,
            total_validator_weight: 200,
        };

        let err = gadget.validate_certificate(&oversized).unwrap_err();
        assert!(
            matches!(&err, BftFinalityError::InvalidCertificate(reason)
                if reason.contains("DoS guard")),
            "expected DoS guard rejection for oversized votes, got {:?}",
            err
        );
    }

    #[test]
    fn submit_vote_verified_rejects_forged_signature() {
        let sk_real = signing_keypair_for(1);
        let sk_attacker = signing_keypair_for(99); // different key
        let validators = vec![FinalityValidator {
            id: "victim".into(),
            stake_weight: 100,
            public_key: Some(sk_real.verifying_key().to_bytes()),
        }];
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(7);
        // Attacker signs a vote claiming to come from "victim".
        let forged = signed_vote(&gadget, &sk_attacker, "victim", 1, hash);
        let err = gadget.submit_vote_verified(forged).unwrap_err();
        assert!(
            matches!(err, BftFinalityError::InvalidSignature(ref id) if id == "victim"),
            "expected InvalidSignature, got {:?}",
            err
        );
    }

    #[test]
    fn submit_vote_verified_refuses_validator_without_pubkey() {
        let validators = vec![FinalityValidator {
            id: "v0".into(),
            stake_weight: 100,
            public_key: None, // no key registered
        }];
        let mut gadget = BftFinalityGadget::new(validators);
        let vote = FinalityVote {
            block_hash: block_hash(1),
            block_height: 1,
            epoch: gadget.epoch(),
            validator_set_hash: gadget.validator_set_hash(),
            validator_id: "v0".into(),
            signature: vec![0u8; 64],
        };
        let err = gadget.submit_vote_verified(vote).unwrap_err();
        assert!(
            matches!(err, BftFinalityError::NoPublicKey(ref id) if id == "v0"),
            "verified path must refuse to fall through to unauthenticated submit_vote, got {:?}",
            err
        );
    }

    #[test]
    fn submit_vote_verified_unknown_validator_returns_unknown() {
        let sk = signing_keypair_for(1);
        let validators = vec![FinalityValidator {
            id: "v0".into(),
            stake_weight: 100,
            public_key: Some(sk.verifying_key().to_bytes()),
        }];
        let mut gadget = BftFinalityGadget::new(validators);
        let vote = signed_vote(&gadget, &sk, "ghost", 1, block_hash(1));
        let err = gadget.submit_vote_verified(vote).unwrap_err();
        assert!(matches!(err, BftFinalityError::UnknownValidator(ref id) if id == "ghost"));
    }

    #[test]
    fn certificate_stays_none_below_threshold_with_weighted_stake() {
        // Weighted set: total 400, supermajority threshold at >266.66
        // (required_weight = 267). Two validators carrying 133 each add up to
        // 266 — one short of the threshold — so finality must NOT trigger.
        let validators = vec![
            FinalityValidator {
                id: "a".into(),
                stake_weight: 133,
                public_key: None,
            },
            FinalityValidator {
                id: "b".into(),
                stake_weight: 133,
                public_key: None,
            },
            FinalityValidator {
                id: "c".into(),
                stake_weight: 134,
                public_key: None,
            },
        ];
        let mut gadget = BftFinalityGadget::new(validators);
        assert_eq!(gadget.total_weight(), 400);
        assert_eq!(gadget.required_weight(), 267);

        let hash = block_hash(11);
        gadget.submit_vote(make_vote("a", 1, hash)).unwrap();
        gadget.submit_vote(make_vote("b", 1, hash)).unwrap();

        assert!(
            !gadget.is_finalized(1),
            "266/400 stake weight must not finalize (below 2/3+1 threshold)"
        );
        assert!(
            gadget.last_certificate().is_none(),
            "no certificate must be produced below supermajority threshold"
        );
    }

    #[test]
    fn large_validator_weights_do_not_overflow_finality_thresholds() {
        let near_max = u64::MAX - 10;
        let validators = vec![
            FinalityValidator {
                id: "whale-0".into(),
                stake_weight: near_max,
                public_key: None,
            },
            FinalityValidator {
                id: "whale-1".into(),
                stake_weight: near_max,
                public_key: None,
            },
            FinalityValidator {
                id: "whale-2".into(),
                stake_weight: 25,
                public_key: None,
            },
        ];
        let mut gadget = BftFinalityGadget::new(validators);
        let hash = block_hash(33);

        let expected_total = u128::from(near_max) + u128::from(near_max) + 25;
        assert_eq!(gadget.total_weight(), expected_total);
        assert_eq!(gadget.required_weight(), (expected_total * 2 / 3) + 1);

        let first = gadget.submit_vote(make_vote("whale-0", 1, hash)).unwrap();
        assert!(matches!(first, FinalityEvent::VoteReceived { .. }));

        let second = gadget.submit_vote(make_vote("whale-1", 1, hash)).unwrap();
        match second {
            FinalityEvent::BlockFinalized {
                total_vote_weight,
                total_validator_weight,
                ..
            } => {
                assert_eq!(total_vote_weight, u128::from(near_max) * 2);
                assert_eq!(total_validator_weight, expected_total);
            },
            other => panic!("expected BlockFinalized, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // #2256: committee binding. A vote is valid only under the committee it was
    // cast in. Guard-revert: remove `ensure_committee_scope` from
    // `submit_vote_verified` and the two rejection tests below go RED.
    // -----------------------------------------------------------------------

    fn keyed_validator(id: &str, sk: &ed25519_dalek::SigningKey) -> FinalityValidator {
        FinalityValidator {
            id: id.into(),
            stake_weight: 100,
            public_key: Some(sk.verifying_key().to_bytes()),
        }
    }

    #[test]
    fn content_hash_binds_epoch_and_validator_set_2256() {
        let base = FinalityVote {
            block_hash: block_hash(1),
            block_height: 1,
            epoch: 0,
            validator_set_hash: [0u8; 32],
            validator_id: "v0".into(),
            signature: Vec::new(),
        };
        let mut diff_epoch = base.clone();
        diff_epoch.epoch = 1;
        let mut diff_set = base.clone();
        diff_set.validator_set_hash = [9u8; 32];

        assert_ne!(
            base.content_hash(),
            diff_epoch.content_hash(),
            "epoch must be bound into the signed preimage"
        );
        assert_ne!(
            base.content_hash(),
            diff_set.content_hash(),
            "validator_set_hash must be bound into the signed preimage"
        );
    }

    #[test]
    fn validator_set_hash_is_order_independent_and_membership_sensitive_2256() {
        let a = FinalityValidator {
            id: "a".into(),
            stake_weight: 10,
            public_key: None,
        };
        let b = FinalityValidator {
            id: "b".into(),
            stake_weight: 20,
            public_key: None,
        };
        // Same members, different order -> same hash (sorted internally).
        assert_eq!(
            compute_validator_set_hash(&[a.clone(), b.clone()]),
            compute_validator_set_hash(&[b.clone(), a.clone()])
        );
        // Different membership -> different hash.
        assert_ne!(
            compute_validator_set_hash(&[a.clone()]),
            compute_validator_set_hash(&[a, b])
        );
    }

    /// Cross-type contract (#2227 + #2256): the committee a `ValidatorSetChange`
    /// declares and the committee a `FinalityVote` binds to must hash to the same
    /// bytes, or a set-change cannot name the set finality votes reference. Both
    /// now route through shared-protocol's `canonical_validator_set_hash`; this
    /// pins that they agree, and goes RED if either re-encodes independently.
    #[test]
    fn validator_set_hash_agrees_with_validator_set_change_2227_2256() {
        use wrfcoin_shared_protocol::consensus::validator_set::{
            ValidatorSetChange, ValidatorSetMember,
        };

        let members = vec![
            ValidatorSetMember {
                validator_id: "v1".into(),
                stake_weight: 200,
                public_key: [2u8; 32],
            },
            ValidatorSetMember {
                validator_id: "v0".into(),
                stake_weight: 100,
                public_key: [1u8; 32],
            },
        ];
        // Deliberately different input order on each side to prove both sort.
        let change = ValidatorSetChange::new(3, 50, members);
        let finality_validators = vec![
            FinalityValidator {
                id: "v0".into(),
                stake_weight: 100,
                public_key: Some([1u8; 32]),
            },
            FinalityValidator {
                id: "v1".into(),
                stake_weight: 200,
                public_key: Some([2u8; 32]),
            },
        ];

        assert_eq!(
            compute_validator_set_hash(&finality_validators),
            *change.validator_set_hash().as_bytes(),
            "ValidatorSetChange (#2227) and the finality committee (#2256) must \
             hash the same validator set to identical bytes"
        );
    }

    #[test]
    fn submit_vote_verified_rejects_vote_from_a_different_committee_2256() {
        // `v0` is a member of BOTH committees, but committee A has 3 validators
        // (300 total power) and B has 2 (200). A vote cast under A must not be
        // countable under B — the #2099 `NoStaleVoteCounted` property.
        let sk0 = signing_keypair_for(1);
        let sk1 = signing_keypair_for(2);
        let sk2 = signing_keypair_for(3);

        let committee_a = vec![
            keyed_validator("v0", &sk0),
            keyed_validator("v1", &sk1),
            keyed_validator("v2", &sk2),
        ];
        let committee_b = vec![keyed_validator("v0", &sk0), keyed_validator("v1", &sk1)];

        let gadget_a = BftFinalityGadget::new(committee_a);
        let mut gadget_b = BftFinalityGadget::new(committee_b);
        assert_ne!(
            gadget_a.validator_set_hash(),
            gadget_b.validator_set_hash(),
            "the two committees must have distinct set hashes"
        );

        // `v0` honestly signs a vote under committee A...
        let vote = signed_vote(&gadget_a, &sk0, "v0", 5, block_hash(7));
        // ...and it is rejected by committee B, which it never named.
        let err = gadget_b.submit_vote_verified(vote).unwrap_err();
        assert!(
            matches!(err, BftFinalityError::WrongCommittee { .. }),
            "a vote cast under one committee must not count under another, got {err:?}"
        );
    }

    #[test]
    fn submit_vote_verified_rejects_wrong_epoch_2256() {
        // Same validator set, different epoch generation. The set hash matches
        // but the epoch does not, so the vote is out of committee.
        let sk0 = signing_keypair_for(1);
        let validators = vec![keyed_validator("v0", &sk0)];

        let gadget_epoch0 = BftFinalityGadget::new(validators.clone());
        let mut gadget_epoch1 = BftFinalityGadget::new(validators).with_epoch(1);
        assert_eq!(
            gadget_epoch0.validator_set_hash(),
            gadget_epoch1.validator_set_hash(),
            "same set -> same set hash; only the epoch differs"
        );

        let vote = signed_vote(&gadget_epoch0, &sk0, "v0", 1, block_hash(1)); // epoch 0
        let err = gadget_epoch1.submit_vote_verified(vote).unwrap_err();
        assert!(
            matches!(err, BftFinalityError::WrongCommittee { .. }),
            "a vote from a different epoch must not count, got {err:?}"
        );
    }

    #[test]
    fn submit_vote_verified_accepts_in_committee_vote_2256() {
        // Positive control: a vote scoped to this gadget's committee passes the
        // scope check (and then the signature check).
        let sk0 = signing_keypair_for(1);
        let sk1 = signing_keypair_for(2);
        let mut gadget = BftFinalityGadget::new(vec![
            keyed_validator("v0", &sk0),
            keyed_validator("v1", &sk1),
        ]);
        let vote = signed_vote(&gadget, &sk0, "v0", 1, block_hash(1));
        assert!(gadget.submit_vote_verified(vote).is_ok());
    }

    #[test]
    fn validate_certificate_rejects_votes_from_a_different_committee_2256() {
        // Two committees of equal size and stake (so the total-weight guard
        // passes) but different members -> different set hash. A certificate
        // whose votes were cast under committee A must not validate against B.
        let sk0 = signing_keypair_for(1);
        let sk1 = signing_keypair_for(2);
        let sk2 = signing_keypair_for(3);
        let sk3 = signing_keypair_for(4);
        let committee_a = vec![
            keyed_validator("v0", &sk0),
            keyed_validator("v1", &sk1),
            keyed_validator("v2", &sk2),
        ];
        let committee_b = vec![
            keyed_validator("v0", &sk0),
            keyed_validator("v1", &sk1),
            keyed_validator("v3", &sk3),
        ];
        let gadget_a = BftFinalityGadget::new(committee_a);
        let gadget_b = BftFinalityGadget::new(committee_b);
        assert_eq!(gadget_a.total_weight(), gadget_b.total_weight());
        assert_ne!(gadget_a.validator_set_hash(), gadget_b.validator_set_hash());

        let hash = block_hash(9);
        let height = 3;
        let cert = FinalityCertificate {
            block_hash: hash,
            block_height: height,
            votes: vec![
                signed_vote(&gadget_a, &sk0, "v0", height, hash),
                signed_vote(&gadget_a, &sk1, "v1", height, hash),
                signed_vote(&gadget_a, &sk2, "v2", height, hash),
            ],
            total_vote_weight: 300,
            total_validator_weight: 300,
        };

        // A validates its own certificate...
        assert!(
            gadget_a.validate_certificate(&cert).is_ok(),
            "committee A must accept a certificate cast under committee A"
        );
        // ...but B rejects it: the votes name A's committee, not B's.
        assert!(
            gadget_b.validate_certificate(&cert).is_err(),
            "a certificate from a different committee must not validate (#2256)"
        );
    }
}
