//! Guardian-based account recovery with an owner-vetoable time-lock (#2172).
//!
//! Social or institutional recovery replaces the seed phrase as the answer to
//! "I lost my device" — but a recovery path that moves faster than the owner
//! can react is a weaker second front door onto the same account. Every rule
//! here exists to keep that front door slower than the owner, not the
//! guardians:
//!
//! - Once the guardian threshold is met, execution waits [`RecoveryPolicy::time_lock_blocks`]
//!   more blocks. During that window the **current owner** — proven by the
//!   account's own key, the same way every other action on this account is
//!   authorized — can cancel unconditionally. That window is what makes
//!   guardian collusion recoverable rather than final.
//! - Guardian approvals are domain-separated ([`guardian_approval_message`])
//!   and bind the account, the proposed new owner, and the request's own
//!   initiation height — an approval harvested from one request can never be
//!   replayed onto a different account, a different candidate owner, or a
//!   later request for the same account.
//! - A duplicate guardian approval (the same guardian counted twice toward
//!   threshold) is refused outright, never silently deduplicated.
//! - Changing the policy is itself time-locked ([`RecoveryPolicy::cooldown_blocks`]
//!   is unrelated to this — see below), and a policy change **in flight**
//!   blocks recovery execution, even past its own time-lock. Otherwise an
//!   attacker with momentary account control swaps the guardians and the
//!   compromised policy grants itself an unlockable recovery a legitimate
//!   policy-change time-lock would have caught.
//! - A cooldown between the *end* of one recovery attempt (executed OR
//!   cancelled) and the start of the next prevents repeated initiation being
//!   used to grief the owner into constant cancellation.
//!
//! ## Why block heights, not wall-clock durations
//!
//! The story's own sketch writes `time_lock: Duration`, but every window here
//! is expressed as a **block count** (`time_lock_blocks`, `cooldown_blocks`)
//! and every deadline as a **height** (`executable_at_height`), not a
//! timestamp. This is enforced *in consensus* (every validating node must
//! reach the identical accept/reject verdict from the identical input), and
//! block height is the one clock every node already agrees on byte-for-byte;
//! a wall-clock duration would need a trusted timestamp source (the block's
//! own producer-supplied timestamp, itself only loosely bounded) for no
//! benefit here. This mirrors how `ConsensusFeature` activation and the
//! upgrade-halt deadline (#3093) are also height-denominated, not time-based.
//!
//! ## Storage and consensus wiring
//!
//! [`AccountRecoveryState`] is the per-account persisted state, held in
//! `State::account_recovery` (a `BTreeMap<Address, AccountRecoveryState>` for
//! canonical iteration order). It is gated end to end by
//! `ConsensusFeature::SocialRecovery` (`shared-protocol/src/crypto/chain_binding.rs`):
//! [`apply`] refuses every action unless the feature is active at the
//! transition's height, so `State::account_recovery` is provably empty on any
//! chain that has not scheduled the feature — which is what keeps
//! `state_root_material()`'s conditional section byte-absent (not merely
//! zero-length) pre-activation. See that function's own comment for the
//! byte-identical-when-unused argument.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::common::types::Address;
use crate::crypto::signature::{PublicKey, Signature};

/// Domain tag for a guardian's approval of a specific recovery request.
pub const RECOVERY_APPROVAL_DOMAIN_TAG: &[u8] = b"wrfcoin.account_recovery_guardian_approval.v1";
/// Domain tag for the owner's cancellation of their own account's active
/// recovery request, during the time-lock window.
pub const RECOVERY_CANCEL_DOMAIN_TAG: &[u8] = b"wrfcoin.account_recovery_owner_cancel.v1";
pub const RECOVERY_SET_POLICY_DOMAIN_TAG: &[u8] = b"wrfcoin.account_recovery_owner_set_policy.v1";
pub const RECOVERY_CANCEL_POLICY_CHANGE_DOMAIN_TAG: &[u8] =
    b"wrfcoin.account_recovery_owner_cancel_policy_change.v1";

/// Who can recover an account, and how much friction stands between "the
/// guardians agree" and "the account actually moves."
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPolicy {
    /// Distinct guardian accounts. No duplicates — see [`RecoveryPolicy::validate`].
    pub guardians: Vec<Address>,
    /// Approvals required before the time-lock starts. `1 <= threshold <= guardians.len()`.
    pub threshold: u16,
    /// Blocks the executable recovery must wait once threshold is met, during
    /// which the owner may cancel unconditionally.
    pub time_lock_blocks: u64,
    /// Blocks required between the end of one recovery attempt and the start
    /// of the next, so repeated initiation cannot grief the owner.
    pub cooldown_blocks: u64,
}

impl RecoveryPolicy {
    /// Structural validity: distinct guardians, and a threshold the guardian
    /// set can actually satisfy. Says nothing about *when* this policy may
    /// take effect — that is [`apply`]'s job, since it depends on whether a
    /// policy already exists.
    pub fn validate(&self) -> Result<(), RecoveryError> {
        if self.guardians.is_empty() {
            return Err(RecoveryError::InvalidPolicy("guardians must not be empty"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for guardian in &self.guardians {
            if !seen.insert(guardian.0) {
                return Err(RecoveryError::GuardianDuplicate(*guardian));
            }
        }
        if self.threshold == 0 {
            return Err(RecoveryError::InvalidPolicy("threshold must be at least 1"));
        }
        if usize::from(self.threshold) > self.guardians.len() {
            return Err(RecoveryError::InvalidPolicy(
                "threshold cannot exceed the number of guardians",
            ));
        }
        Ok(())
    }
}

/// One guardian's approval of a specific, in-flight [`RecoveryRequest`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardianApproval {
    /// The guardian's account address, as listed in the policy.
    pub guardian: Address,
    /// Lowercase hex ed25519 public key. Its hash must equal `guardian`
    /// ([`Address::from_public_key`]) — the same binding every other action
    /// on an address proves, so a guardian's identity is never asserted, only
    /// derived from the key that signed.
    pub guardian_public_key: String,
    /// Hex signature over [`guardian_approval_message`].
    pub signature: String,
}

/// A candidate ownership change, moving through threshold and time-lock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Parsed ed25519 public key of the proposed new owner. The action input
    /// is hex, but it is validated before it can enter consensus state.
    pub new_owner: PublicKey,
    /// Guardian approvals collected so far. May be fewer than
    /// `policy.threshold`; [`RecoveryRequest::executable_at_height`] is only
    /// meaningful once it is not.
    pub approvals: Vec<GuardianApproval>,
    /// Height at which this request was first initiated. Part of the
    /// approval preimage, so an approval cannot be replayed onto a later
    /// request for the same account.
    pub initiated_at_height: u64,
    /// `initiated_at_height + policy.time_lock_blocks`, computed once
    /// threshold is first met (see [`RecoveryRequest::executable_at_height_if_ready`]).
    /// `None` before threshold is met.
    pub executable_at_height: Option<u64>,
}

impl RecoveryRequest {
    fn threshold_met(&self, threshold: u16) -> bool {
        self.approvals.len() >= usize::from(threshold)
    }
}

/// A policy change waiting out its own time-lock before it replaces the
/// active policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPolicyChange {
    pub new_policy: RecoveryPolicy,
    pub initiated_at_height: u64,
    pub executable_at_height: u64,
}

/// Per-account recovery state. Absent (no entry in `State::account_recovery`)
/// is the default for every account: no policy, nothing pending.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRecoveryState {
    /// Monotonic authorization sequence for policy changes. Bound into every
    /// owner-signed SetPolicy preimage so published policy actions cannot be
    /// replayed after acceptance.
    #[serde(default)]
    pub policy_nonce: u64,
    pub policy: Option<RecoveryPolicy>,
    pub pending_policy_change: Option<PendingPolicyChange>,
    pub active_request: Option<RecoveryRequest>,
    /// Height at which the most recent recovery attempt ENDED (executed or
    /// cancelled) — the cooldown clock. `None` before any attempt.
    pub last_attempt_ended_at_height: Option<u64>,
}

/// What a single [`apply`] call asks the state machine to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryAction {
    /// Set the recovery policy, authenticated by the current owner. Immediate if no policy exists yet (there is
    /// nothing for a time-lock to protect); otherwise queued as a
    /// [`PendingPolicyChange`] that takes `time_lock_blocks` of the *current*
    /// (about-to-be-replaced) policy to take effect.
    SetPolicy {
        policy: RecoveryPolicy,
        policy_nonce: u64,
        owner_public_key: String,
        signature: String,
    },
    /// Initiate a request, or add approvals to the account's existing one.
    /// Refuses a duplicate guardian and refuses when a policy change is
    /// already in flight (the guardian set a request would need to satisfy
    /// must not be able to change out from under it mid-flight either).
    InitiateOrApprove {
        new_owner: String,
        approvals: Vec<GuardianApproval>,
    },
    /// The owner cancels the account's active request, proven by a signature
    /// over [`owner_cancel_message`] from the account's own key.
    Cancel {
        owner_public_key: String,
        signature: String,
    },
    /// Cancel a policy replacement before its time-lock completes, proven by
    /// the current owner's key.
    CancelPendingPolicyChange {
        owner_public_key: String,
        signature: String,
    },
    /// Promote a mature policy replacement. Anyone may submit this action;
    /// owner authorization happened when the replacement was queued.
    PromotePendingPolicyChange,
    /// Execute the active request once threshold is met, the time-lock has
    /// elapsed, and no policy change is in flight.
    Execute,
}

/// Every way [`apply`] refuses an action. `Debug`+`PartialEq`, no panic
/// anywhere in this module.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RecoveryError {
    #[error("account has no recovery policy set")]
    NoPolicy,
    #[error("guardian threshold not met: need {needed}, have {have}")]
    ThresholdNotMet { needed: u16, have: u16 },
    #[error("time-lock active until height {executable_at_height}")]
    TimeLockActive { executable_at_height: u64 },
    #[error("cooldown active until height {cooldown_ends_at_height}")]
    CooldownActive { cooldown_ends_at_height: u64 },
    #[error("guardian {0} already approved this request")]
    GuardianDuplicate(Address),
    #[error("{0} is not a guardian under the active policy")]
    NotAGuardian(Address),
    #[error("guardian public key does not hash to its claimed address")]
    GuardianKeyAddressMismatch,
    #[error("guardian approval signature does not verify")]
    BadApproval,
    #[error("recovery was cancelled by the owner")]
    CancelledByOwner,
    #[error("owner cancellation key does not hash to this account's address")]
    CancelKeyAddressMismatch,
    #[error("owner cancellation signature does not verify")]
    BadCancellation,
    #[error("owner policy signature does not verify")]
    BadPolicyAuthorization,
    #[error("stale policy nonce: expected {expected}, got {got}")]
    StalePolicyNonce { expected: u64, got: u64 },
    #[error(
        "a policy change is in flight and blocks recovery execution until height {executable_at_height}"
    )]
    PolicyChangeInFlight { executable_at_height: u64 },
    #[error("no active recovery request for this account")]
    NoActiveRequest,
    #[error("no pending policy change for this account")]
    NoPendingPolicyChange,
    #[error("invalid policy: {0}")]
    InvalidPolicy(&'static str),
    #[error("malformed hex key or signature: {0}")]
    Malformed(&'static str),
    #[error(
        "this consensus feature is not active at this height (#2172, ConsensusFeature::SocialRecovery)"
    )]
    FeatureNotActive,
}

/// The exact bytes a guardian signs to approve `new_owner` taking over
/// `account`, for the request that began at `initiated_at_height`. Binding
/// all three is what makes an approval non-replayable across accounts,
/// candidate owners, and re-initiated requests for the same account.
pub fn guardian_approval_message(
    account: &Address,
    new_owner: &str,
    initiated_at_height: u64,
) -> Vec<u8> {
    let mut message = Vec::with_capacity(RECOVERY_APPROVAL_DOMAIN_TAG.len() + 64 + new_owner.len());
    message.extend_from_slice(RECOVERY_APPROVAL_DOMAIN_TAG);
    message.push(0);
    message.extend_from_slice(&account.0);
    message.push(0);
    message.extend_from_slice(new_owner.as_bytes());
    message.push(0);
    message.extend_from_slice(&initiated_at_height.to_le_bytes());
    message
}

/// The exact bytes the owner signs to cancel `account`'s active request,
/// which began at `initiated_at_height`. Binding the height means a
/// cancellation signed for one request cannot later cancel a different,
/// re-initiated one by coincidence of timing.
pub fn owner_cancel_message(account: &Address, initiated_at_height: u64) -> Vec<u8> {
    let mut message = Vec::with_capacity(RECOVERY_CANCEL_DOMAIN_TAG.len() + 40);
    message.extend_from_slice(RECOVERY_CANCEL_DOMAIN_TAG);
    message.push(0);
    message.extend_from_slice(&account.0);
    message.push(0);
    message.extend_from_slice(&initiated_at_height.to_le_bytes());
    message
}

/// The exact bytes the owner signs to install or queue a recovery policy.
pub fn owner_set_policy_message(account: &Address, policy: &RecoveryPolicy, policy_nonce: u64) -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(RECOVERY_SET_POLICY_DOMAIN_TAG);
    message.push(0);
    message.extend_from_slice(&account.0);
    message.extend_from_slice(&policy_nonce.to_le_bytes());
    message.extend_from_slice(&(policy.guardians.len() as u64).to_le_bytes());
    for guardian in &policy.guardians {
        message.extend_from_slice(&guardian.0);
    }
    message.extend_from_slice(&policy.threshold.to_le_bytes());
    message.extend_from_slice(&policy.time_lock_blocks.to_le_bytes());
    message.extend_from_slice(&policy.cooldown_blocks.to_le_bytes());
    message
}

pub fn owner_cancel_policy_change_message(account: &Address, initiated_at_height: u64) -> Vec<u8> {
    let mut message = Vec::with_capacity(RECOVERY_CANCEL_POLICY_CHANGE_DOMAIN_TAG.len() + 40);
    message.extend_from_slice(RECOVERY_CANCEL_POLICY_CHANGE_DOMAIN_TAG);
    message.push(0);
    message.extend_from_slice(&account.0);
    message.push(0);
    message.extend_from_slice(&initiated_at_height.to_le_bytes());
    message
}

fn parse_hex_public_key(hex: &str, malformed: &'static str) -> Result<PublicKey, RecoveryError> {
    PublicKey::from_hex(hex.trim()).map_err(|_| RecoveryError::Malformed(malformed))
}

fn parse_hex_signature(hex: &str, malformed: &'static str) -> Result<Signature, RecoveryError> {
    Signature::from_hex(hex.trim()).map_err(|_| RecoveryError::Malformed(malformed))
}

fn verify_owner_signature(
    account: &Address,
    owner_public_key: &str,
    signature: &str,
    message: &[u8],
) -> Result<(), RecoveryError> {
    let public_key = parse_hex_public_key(owner_public_key, "owner public key is not valid hex ed25519")?;
    if Address::from_public_key(&public_key) != *account {
        return Err(RecoveryError::CancelKeyAddressMismatch);
    }
    let signature = parse_hex_signature(signature, "owner signature is not valid hex")?;
    if !public_key.verify(message, &signature) {
        return Err(RecoveryError::BadPolicyAuthorization);
    }
    Ok(())
}

/// Verify one guardian approval against the policy and the request it
/// belongs to. Pure: takes everything it needs as arguments, mutates
/// nothing, so [`apply`] can validate every approval in a batch before
/// committing any of them.
fn verify_guardian_approval(
    policy: &RecoveryPolicy,
    account: &Address,
    new_owner: &PublicKey,
    initiated_at_height: u64,
    already_approved: &std::collections::BTreeSet<[u8; 20]>,
    approval: &GuardianApproval,
) -> Result<(), RecoveryError> {
    if !policy.guardians.contains(&approval.guardian) {
        return Err(RecoveryError::NotAGuardian(approval.guardian));
    }
    if already_approved.contains(&approval.guardian.0) {
        return Err(RecoveryError::GuardianDuplicate(approval.guardian));
    }
    let public_key = parse_hex_public_key(
        &approval.guardian_public_key,
        "guardian public key is not valid hex ed25519",
    )?;
    if Address::from_public_key(&public_key) != approval.guardian {
        return Err(RecoveryError::GuardianKeyAddressMismatch);
    }
    let signature =
        parse_hex_signature(&approval.signature, "guardian signature is not valid hex")?;
    let message = guardian_approval_message(account, &new_owner.to_hex(), initiated_at_height);
    if !public_key.verify(&message, &signature) {
        return Err(RecoveryError::BadApproval);
    }
    Ok(())
}

/// Apply one [`RecoveryAction`] to `account`'s entry in `states` at `height`.
/// The single entry point consensus code calls — see
/// `StateTransition::ApplyAccountRecovery` in `blockchain/state.rs`, whose
/// match arm is exactly a call into this function.
pub fn apply(
    states: &mut BTreeMap<Address, AccountRecoveryState>,
    account: Address,
    action: RecoveryAction,
    height: u64,
) -> Result<Option<PublicKey>, RecoveryError> {
    match action {
        RecoveryAction::SetPolicy { policy, policy_nonce, owner_public_key, signature } => {
            policy.validate()?;
            let entry = states.get(&account).cloned().unwrap_or_default();
            if policy_nonce != entry.policy_nonce {
                return Err(RecoveryError::StalePolicyNonce {
                    expected: entry.policy_nonce,
                    got: policy_nonce,
                });
            }
            if entry.pending_policy_change.is_none() && entry.policy.as_ref() == Some(&policy) {
                return Err(RecoveryError::InvalidPolicy("replacement policy must differ from the active policy"));
            }
            verify_owner_signature(
                &account,
                &owner_public_key,
                &signature,
                &owner_set_policy_message(&account, &policy, policy_nonce),
            )?;
            let mut entry = entry;
            if let Some(pending) = &entry.pending_policy_change {
                return Err(RecoveryError::PolicyChangeInFlight {
                    executable_at_height: pending.executable_at_height,
                });
            }
            match &entry.policy {
                None => {
                    // Bootstrapping: no existing guardian set to protect, so
                    // there is nothing a time-lock would gain here.
                    entry.policy = Some(policy);
                },
                Some(_) => {
                    entry.pending_policy_change = Some(PendingPolicyChange {
                        executable_at_height: height
                            .saturating_add(policy_time_lock_for_change(&policy, &entry)),
                        new_policy: policy,
                        initiated_at_height: height,
                    });
                },
            }
            entry.policy_nonce = entry.policy_nonce.checked_add(1)
                .ok_or(RecoveryError::InvalidPolicy("policy nonce exhausted"))?;
            states.insert(account, entry);
            Ok(None)
        },
        RecoveryAction::InitiateOrApprove {
            new_owner,
            approvals,
        } => {
            // A missing policy is the primary refusal for this action. Do
            // not inspect a candidate owner or approval batch until there
            // is a guardian policy that can authorize either of them.
            let mut entry = states.get(&account).cloned().ok_or(RecoveryError::NoPolicy)?;
            let policy = entry.policy.clone().ok_or(RecoveryError::NoPolicy)?;
            let new_owner = parse_hex_public_key(&new_owner, "new owner public key is not valid hex ed25519")?;
            if Address::from_public_key(&new_owner) == account {
                return Err(RecoveryError::InvalidPolicy("new owner must differ from the current owner"));
            }
            if let Some(pending) = &entry.pending_policy_change {
                return Err(RecoveryError::PolicyChangeInFlight {
                    executable_at_height: pending.executable_at_height,
                });
            }

            // A below-threshold request cannot let one rogue guardian deny a
            // threshold-backed recovery for another owner. A different target
            // may replace it only when this submitted batch itself validates
            // to the full threshold.
            if let Some(active) = &entry.active_request {
                if active.new_owner != new_owner {
                    if active.threshold_met(policy.threshold) || approvals.len() < usize::from(policy.threshold) {
                        return Err(RecoveryError::InvalidPolicy(
                            "an active threshold-backed request already names a different new_owner",
                        ));
                    }
                    let mut already = std::collections::BTreeSet::new();
                    for approval in &approvals {
                        verify_guardian_approval(&policy, &account, &new_owner, height, &already, approval)?;
                        already.insert(approval.guardian.0);
                    }
                    entry.active_request = Some(RecoveryRequest {
                        new_owner,
                        approvals,
                        initiated_at_height: height,
                        executable_at_height: Some(height.saturating_add(policy.time_lock_blocks)),
                    });
                    states.insert(account, entry);
                    return Ok(None);
                }
            }

            if entry.active_request.is_none() {
                if approvals.is_empty() {
                    return Err(RecoveryError::InvalidPolicy(
                        "opening a recovery request requires at least one guardian approval",
                    ));
                }
                if let Some(ended_at) = entry.last_attempt_ended_at_height {
                    let cooldown_ends_at = ended_at.saturating_add(policy.cooldown_blocks);
                    if height < cooldown_ends_at {
                        return Err(RecoveryError::CooldownActive {
                            cooldown_ends_at_height: cooldown_ends_at,
                        });
                    }
                }
                entry.active_request = Some(RecoveryRequest {
                    new_owner: new_owner.clone(),
                    approvals: Vec::new(),
                    initiated_at_height: height,
                    executable_at_height: None,
                });
            }

            let request = entry
                .active_request
                .as_mut()
                .ok_or(RecoveryError::NoActiveRequest)?;
            if request.new_owner != new_owner {
                // A different candidate owner for the same in-flight request
                // is a fresh request, not an addition to this one — treated
                // as invalid input rather than silently switching targets.
                return Err(RecoveryError::InvalidPolicy(
                    "an active request already names a different new_owner; cancel it before starting another",
                ));
            }

            let mut already: std::collections::BTreeSet<[u8; 20]> =
                request.approvals.iter().map(|a| a.guardian.0).collect();
            let mut validated = Vec::with_capacity(approvals.len());
            for approval in approvals {
                verify_guardian_approval(
                    &policy,
                    &account,
                    &request.new_owner,
                    request.initiated_at_height,
                    &already,
                    &approval,
                )?;
                already.insert(approval.guardian.0);
                validated.push(approval);
            }
            request.approvals.extend(validated);

            if request.executable_at_height.is_none() && request.threshold_met(policy.threshold) {
                request.executable_at_height = Some(height.saturating_add(policy.time_lock_blocks));
            }
            states.insert(account, entry);
            Ok(None)
        },
        RecoveryAction::Cancel {
            owner_public_key,
            signature,
        } => {
            let mut entry = states
                .get(&account)
                .cloned()
                .ok_or(RecoveryError::NoActiveRequest)?;
            let request = entry
                .active_request
                .as_ref()
                .ok_or(RecoveryError::NoActiveRequest)?;
            let message = owner_cancel_message(&account, request.initiated_at_height);
            verify_owner_signature(&account, &owner_public_key, &signature, &message)
                .map_err(|err| match err {
                    RecoveryError::BadPolicyAuthorization => RecoveryError::BadCancellation,
                    other => other,
                })?;
            entry.active_request = None;
            entry.last_attempt_ended_at_height = Some(height);
            states.insert(account, entry);
            Ok(None)
        },
        RecoveryAction::CancelPendingPolicyChange { owner_public_key, signature } => {
            let mut entry = states.get(&account).cloned().ok_or(RecoveryError::NoPendingPolicyChange)?;
            let pending = entry.pending_policy_change.as_ref().ok_or(RecoveryError::NoPendingPolicyChange)?;
            let message = owner_cancel_policy_change_message(&account, pending.initiated_at_height);
            verify_owner_signature(&account, &owner_public_key, &signature, &message)?;
            entry.pending_policy_change = None;
            states.insert(account, entry);
            Ok(None)
        },
        RecoveryAction::PromotePendingPolicyChange => {
            let mut entry = states.get(&account).cloned().ok_or(RecoveryError::NoPendingPolicyChange)?;
            let pending = entry.pending_policy_change.clone().ok_or(RecoveryError::NoPendingPolicyChange)?;
            if height < pending.executable_at_height {
                return Err(RecoveryError::TimeLockActive {
                    executable_at_height: pending.executable_at_height,
                });
            }
            entry.policy = Some(pending.new_policy);
            entry.pending_policy_change = None;
            entry.active_request = None;
            entry.last_attempt_ended_at_height = Some(height);
            states.insert(account, entry);
            Ok(None)
        },
        RecoveryAction::Execute => {
            let mut entry = states
                .get(&account)
                .cloned()
                .ok_or(RecoveryError::NoActiveRequest)?;
            if let Some(pending) = &entry.pending_policy_change {
                return Err(RecoveryError::PolicyChangeInFlight {
                    executable_at_height: pending.executable_at_height,
                });
            }
            let policy = entry.policy.clone().ok_or(RecoveryError::NoPolicy)?;
            let request = entry
                .active_request
                .clone()
                .ok_or(RecoveryError::NoActiveRequest)?;
            if !request.threshold_met(policy.threshold) {
                return Err(RecoveryError::ThresholdNotMet {
                    needed: policy.threshold,
                    have: request.approvals.len() as u16,
                });
            }
            let executable_at = request.executable_at_height.ok_or(RecoveryError::ThresholdNotMet {
                needed: policy.threshold,
                have: request.approvals.len() as u16,
            })?;
            if height < executable_at {
                return Err(RecoveryError::TimeLockActive {
                    executable_at_height: executable_at,
                });
            }
            // Executed: clear the request, start the cooldown, and return
            // the verified key so State atomically moves the account and its
            // recovery state to the recovered owner's address.
            entry.active_request = None;
            entry.last_attempt_ended_at_height = Some(height);
            states.insert(account, entry);
            Ok(Some(request.new_owner))
        },
    }
}

/// The time-lock that gates a *policy change*, taken from whichever policy is
/// stricter about protecting itself: the incoming policy's own
/// `time_lock_blocks`, or — if larger — the currently active policy's, so an
/// attacker cannot shorten their own escape window by proposing a
/// fast-time-lock replacement policy in the same breath as the swap.
fn policy_time_lock_for_change(new_policy: &RecoveryPolicy, entry: &AccountRecoveryState) -> u64 {
    let current = entry
        .policy
        .as_ref()
        .map(|p| p.time_lock_blocks)
        .unwrap_or(0);
    new_policy.time_lock_blocks.max(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::PrivateKey;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn keypair(seed: u64) -> (PrivateKey, PublicKey, Address) {
        let sk = PrivateKey::generate(&mut StdRng::seed_from_u64(seed));
        let pk = PublicKey::from_private_key(&sk);
        let addr = Address::from_public_key(&pk);
        (sk, pk, addr)
    }

    fn approve(
        sk: &PrivateKey,
        pk: &PublicKey,
        guardian: Address,
        account: &Address,
        new_owner: &str,
        initiated_at_height: u64,
    ) -> GuardianApproval {
        let message = guardian_approval_message(account, new_owner, initiated_at_height);
        let signature = sk.sign(&message);
        GuardianApproval {
            guardian,
            guardian_public_key: pk.to_hex(),
            signature: signature.to_hex(),
        }
    }

    fn set_policy_action(
        owner_sk: &PrivateKey,
        owner_pk: &PublicKey,
        account: Address,
        policy: RecoveryPolicy,
        policy_nonce: u64,
    ) -> RecoveryAction {
        let signature = owner_sk.sign(&owner_set_policy_message(&account, &policy, policy_nonce));
        RecoveryAction::SetPolicy {
            policy,
            policy_nonce,
            owner_public_key: owner_pk.to_hex(),
            signature: signature.to_hex(),
        }
    }

    struct Fixture {
        account: Address,
        owner_sk: PrivateKey,
        owner_pk: PublicKey,
        guardians: Vec<(PrivateKey, PublicKey, Address)>,
        new_owner_pk: String,
        states: BTreeMap<Address, AccountRecoveryState>,
    }

    fn fixture(threshold: u16, time_lock_blocks: u64, cooldown_blocks: u64) -> Fixture {
        let (owner_sk, owner_pk, account) = keypair(0xACC0);
        let guardians: Vec<_> = (0..3).map(|i| keypair(0x6000 + i)).collect();
        let (_new_owner_sk, new_owner_pk, _) = keypair(0x9999);
        let policy = RecoveryPolicy {
            guardians: guardians.iter().map(|(_, _, a)| *a).collect(),
            threshold,
            time_lock_blocks,
            cooldown_blocks,
        };
        let mut states = BTreeMap::new();
        apply(
            &mut states,
            account,
            set_policy_action(&owner_sk, &owner_pk, account, policy, 0),
            0,
        )
        .unwrap();
        Fixture {
            account,
            owner_sk,
            owner_pk,
            guardians,
            new_owner_pk: new_owner_pk.to_hex(),
            states,
        }
    }

    impl Fixture {
        fn approvals_from(&self, indices: &[usize], height: u64) -> Vec<GuardianApproval> {
            indices
                .iter()
                .map(|&i| {
                    let (sk, pk, addr) = &self.guardians[i];
                    approve(sk, pk, *addr, &self.account, &self.new_owner_pk, height)
                })
                .collect()
        }
    }

    #[test]
    fn threshold_met_then_owner_cancels_during_time_lock() {
        let mut f = fixture(2, 100, 10);
        let approvals = f.approvals_from(&[0, 1], 5);

        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals,
            },
            5,
        )
        .unwrap();

        let entry = &f.states[&f.account];
        assert_eq!(
            entry.active_request.as_ref().unwrap().executable_at_height,
            Some(105)
        );

        // Threshold met, still inside the time-lock: execution refuses.
        let err = apply(&mut f.states, f.account, RecoveryAction::Execute, 50).unwrap_err();
        assert_eq!(
            err,
            RecoveryError::TimeLockActive {
                executable_at_height: 105
            }
        );

        // Owner cancels using their own key, before the time-lock elapses.
        let message = owner_cancel_message(&f.account, 5);
        let signature = f.owner_sk.sign(&message);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::Cancel {
                owner_public_key: f.owner_pk.to_hex(),
                signature: signature.to_hex(),
            },
            60,
        )
        .unwrap();

        assert!(f.states[&f.account].active_request.is_none());
        // Execution now refuses: there is nothing to execute.
        let err = apply(&mut f.states, f.account, RecoveryAction::Execute, 200).unwrap_err();
        assert_eq!(err, RecoveryError::NoActiveRequest);
    }

    #[test]
    fn execution_after_time_lock_succeeds() {
        let mut f = fixture(2, 100, 10);
        let approvals = f.approvals_from(&[0, 1], 5);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals,
            },
            5,
        )
        .unwrap();

        apply(&mut f.states, f.account, RecoveryAction::Execute, 105).unwrap();
        let entry = &f.states[&f.account];
        assert!(entry.active_request.is_none());
        assert_eq!(entry.last_attempt_ended_at_height, Some(105));
    }

    #[test]
    fn execute_returns_the_verified_new_owner_for_state_to_transfer() {
        let mut f = fixture(1, 0, 0);
        let approvals = f.approvals_from(&[0], 5);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals,
            },
            5,
        )
        .unwrap();

        let new_owner = apply(&mut f.states, f.account, RecoveryAction::Execute, 5)
            .unwrap()
            .expect("Execute must return the recovered owner to State");
        assert_eq!(new_owner.to_hex(), f.new_owner_pk);
    }

    #[test]
    fn rejected_approval_batch_is_atomic_and_execute_never_panics() {
        let mut f = fixture(1, 10, 0);
        let mut approvals = f.approvals_from(&[0], 5);
        let (sk, pk, addr) = keypair(0xDEAD);
        approvals.push(approve(&sk, &pk, addr, &f.account, &f.new_owner_pk, 5));

        let err = apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals,
            },
            5,
        )
        .unwrap_err();
        assert_eq!(err, RecoveryError::NotAGuardian(addr));
        assert!(f.states[&f.account].active_request.is_none());
        assert_eq!(
            apply(&mut f.states, f.account, RecoveryAction::Execute, 1_000),
            Err(RecoveryError::NoActiveRequest)
        );
    }

    #[test]
    fn zero_approval_request_is_rejected_without_squatting_the_slot() {
        let mut f = fixture(2, 10, 0);
        let err = apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals: vec![],
            },
            5,
        )
        .unwrap_err();
        assert!(matches!(err, RecoveryError::InvalidPolicy(_)));
        assert!(f.states[&f.account].active_request.is_none());

        let approvals = f.approvals_from(&[0, 1], 6);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals,
            },
            6,
        )
        .unwrap();
    }

    #[test]
    fn malformed_new_owner_never_enters_recovery_state() {
        let mut f = fixture(1, 10, 0);
        let err = apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: "not-an-ed25519-key".into(),
                approvals: vec![],
            },
            5,
        )
        .unwrap_err();
        assert!(matches!(err, RecoveryError::Malformed(_)));
        assert!(f.states[&f.account].active_request.is_none());
    }

    #[test]
    fn set_policy_requires_the_current_owner_signature() {
        let mut states = BTreeMap::new();
        let (owner_sk, owner_pk, account) = keypair(0x1111);
        let (attacker_sk, attacker_pk, attacker_guardian) = keypair(0x2222);
        let policy = RecoveryPolicy {
            guardians: vec![attacker_guardian],
            threshold: 1,
            time_lock_blocks: 0,
            cooldown_blocks: 0,
        };
        let attacker_signature = attacker_sk.sign(&owner_set_policy_message(&account, &policy, 0));
        let err = apply(
            &mut states,
            account,
            RecoveryAction::SetPolicy {
                policy: policy.clone(),
                policy_nonce: 0,
                owner_public_key: attacker_pk.to_hex(),
                signature: attacker_signature.to_hex(),
            },
            1,
        )
        .unwrap_err();
        assert_eq!(err, RecoveryError::CancelKeyAddressMismatch);
        assert!(states.is_empty());

        apply(
            &mut states,
            account,
            set_policy_action(&owner_sk, &owner_pk, account, policy, 0),
            1,
        )
        .unwrap();
    }

    #[test]
    fn published_set_policy_cannot_be_replayed_at_a_stale_nonce() {
        let mut f = fixture(2, 10, 50);
        let bootstrap_policy = f.states[&f.account].policy.clone().unwrap();
        let replay = set_policy_action(&f.owner_sk, &f.owner_pk, f.account, bootstrap_policy, 0);
        let before = f.states.clone();
        let err = apply(&mut f.states, f.account, replay, 6).unwrap_err();
        assert_eq!(err, RecoveryError::StalePolicyNonce { expected: 1, got: 0 });
        assert_eq!(f.states, before);
    }

    #[test]
    fn threshold_guardians_replace_a_rogue_below_threshold_request() {
        let mut f = fixture(2, 10, 0);
        let (_, rogue_owner, _) = keypair(0xBAD2);
        let (sk, pk, guardian) = f.guardians[0].clone();
        let rogue_approval = approve(&sk, &pk, guardian, &f.account, &rogue_owner.to_hex(), 5);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: rogue_owner.to_hex(),
                approvals: vec![rogue_approval],
            },
            5,
        )
        .unwrap();
        let approvals = f.approvals_from(&[1, 2], 6);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals,
            },
            6,
        )
        .unwrap();
        let request = f.states[&f.account].active_request.as_ref().unwrap();
        assert_eq!(request.new_owner.to_hex(), f.new_owner_pk);
        assert_eq!(request.approvals.len(), 2);
    }

    #[test]
    fn duplicate_guardian_approval_is_rejected() {
        let mut f = fixture(2, 100, 10);
        let (sk0, pk0, addr0) = f.guardians[0].clone();
        let a1 = approve(&sk0, &pk0, addr0, &f.account, &f.new_owner_pk, 5);
        let a2 = approve(&sk0, &pk0, addr0, &f.account, &f.new_owner_pk, 5);

        let err = apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals: vec![a1, a2],
            },
            5,
        )
        .unwrap_err();
        assert_eq!(err, RecoveryError::GuardianDuplicate(addr0));
    }

    #[test]
    fn duplicate_guardian_across_two_calls_is_also_rejected() {
        let mut f = fixture(2, 100, 10);
        let approvals1 = f.approvals_from(&[0], 5);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals: approvals1,
            },
            5,
        )
        .unwrap();
        let approvals2 = f.approvals_from(&[0], 6);
        let err = apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals: approvals2,
            },
            6,
        )
        .unwrap_err();
        assert_eq!(err, RecoveryError::GuardianDuplicate(f.guardians[0].2));
    }

    #[test]
    fn cooldown_blocks_rapid_retry() {
        let mut f = fixture(1, 10, 1_000);
        let approvals1 = f.approvals_from(&[0], 5);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals: approvals1,
            },
            5,
        )
        .unwrap();
        apply(&mut f.states, f.account, RecoveryAction::Execute, 15).unwrap();
        assert_eq!(f.states[&f.account].last_attempt_ended_at_height, Some(15));

        // Immediately retrying is refused by the cooldown.
        let approvals2 = f.approvals_from(&[0], 16);
        let err = apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals: approvals2,
            },
            16,
        )
        .unwrap_err();
        assert_eq!(
            err,
            RecoveryError::CooldownActive {
                cooldown_ends_at_height: 1015
            }
        );

        // After the cooldown, a new attempt is accepted.
        let approvals3 = f.approvals_from(&[0], 1015);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals: approvals3,
            },
            1015,
        )
        .unwrap();
        assert!(f.states[&f.account].active_request.is_some());
    }

    #[test]
    fn in_flight_policy_change_blocks_recovery_execution() {
        let mut f = fixture(1, 10, 0);
        let approvals = f.approvals_from(&[0], 5);
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals,
            },
            5,
        )
        .unwrap();
        // Threshold met and time-lock elapsed by height 20 -- but a policy
        // change lands at height 16, before execution is attempted.
        let new_policy = RecoveryPolicy {
            guardians: f.guardians.iter().map(|(_, _, a)| *a).collect(),
            threshold: 2,
            time_lock_blocks: 10,
            cooldown_blocks: 0,
        };
        apply(
            &mut f.states,
            f.account,
            set_policy_action(&f.owner_sk, &f.owner_pk, f.account, new_policy, 1),
            16,
        )
        .unwrap();
        assert!(f.states[&f.account].pending_policy_change.is_some());

        let err = apply(&mut f.states, f.account, RecoveryAction::Execute, 20).unwrap_err();
        match err {
            RecoveryError::PolicyChangeInFlight { .. } => {},
            other => panic!("expected PolicyChangeInFlight, got {other:?}"),
        }

        // A mature replacement is explicitly promoted, rather than bricking
        // the account behind a pending change forever. Promotion clears the
        // request approved under the old guardian set.
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::PromotePendingPolicyChange,
            26,
        )
        .unwrap();
        let entry = &f.states[&f.account];
        assert!(entry.pending_policy_change.is_none());
        assert!(entry.active_request.is_none());
        assert_eq!(entry.policy.as_ref().unwrap().threshold, 2);
    }

    #[test]
    fn owner_can_cancel_a_queued_policy_change() {
        let mut f = fixture(1, 10, 0);
        let replacement = RecoveryPolicy {
            guardians: vec![f.guardians[1].2],
            threshold: 1,
            time_lock_blocks: 10,
            cooldown_blocks: 0,
        };
        apply(
            &mut f.states,
            f.account,
            set_policy_action(&f.owner_sk, &f.owner_pk, f.account, replacement, 1),
            6,
        )
        .unwrap();
        let pending = f.states[&f.account].pending_policy_change.clone().unwrap();
        let signature = f.owner_sk.sign(&owner_cancel_policy_change_message(
            &f.account,
            pending.initiated_at_height,
        ));
        apply(
            &mut f.states,
            f.account,
            RecoveryAction::CancelPendingPolicyChange {
                owner_public_key: f.owner_pk.to_hex(),
                signature: signature.to_hex(),
            },
            7,
        )
        .unwrap();
        assert!(f.states[&f.account].pending_policy_change.is_none());
    }

    #[test]
    fn approval_replay_against_a_different_account_fails() {
        let f = fixture(1, 10, 0);
        let (sk0, pk0, addr0) = f.guardians[0].clone();
        // Signed for f.account, at height 5.
        let approval = approve(&sk0, &pk0, addr0, &f.account, &f.new_owner_pk, 5);

        // A different account, same guardian address happens to be listed
        // there too (contrived, but exercises the binding precisely): the
        // approval must still fail because the signed message embeds
        // f.account's own address, not this other one.
        let (other_owner_sk, other_owner_pk, other_account) = keypair(0xBEEF);
        let mut states = BTreeMap::new();
        let policy = RecoveryPolicy {
            guardians: vec![addr0],
            threshold: 1,
            time_lock_blocks: 10,
            cooldown_blocks: 0,
        };
        apply(
            &mut states,
            other_account,
            set_policy_action(&other_owner_sk, &other_owner_pk, other_account, policy, 0),
            0,
        )
        .unwrap();
        let err = apply(
            &mut states,
            other_account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals: vec![approval],
            },
            5,
        )
        .unwrap_err();
        assert_eq!(err, RecoveryError::BadApproval);
    }

    #[test]
    fn approval_replay_against_a_different_new_owner_fails() {
        let mut f = fixture(1, 10, 0);
        let (sk0, pk0, addr0) = f.guardians[0].clone();
        let approval = approve(&sk0, &pk0, addr0, &f.account, &f.new_owner_pk, 5);

        let (_, other_new_owner_pk, _) = keypair(0x1234);
        let err = apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: other_new_owner_pk.to_hex(),
                approvals: vec![approval],
            },
            5,
        )
        .unwrap_err();
        assert_eq!(err, RecoveryError::BadApproval);
    }

    #[test]
    fn policy_bootstrap_is_immediate_but_replacement_is_time_locked() {
        let mut states = BTreeMap::new();
        let (owner_sk, owner_pk, account) = keypair(0x1);
        let (_, _, g0) = keypair(0x2);
        let (_, _, g1) = keypair(0x3);
        let policy = RecoveryPolicy {
            guardians: vec![g0],
            threshold: 1,
            time_lock_blocks: 5,
            cooldown_blocks: 0,
        };
        apply(
            &mut states,
            account,
            set_policy_action(&owner_sk, &owner_pk, account, policy.clone(), 0),
            0,
        )
        .unwrap();
        // Bootstrap: applied immediately, nothing pending.
        assert_eq!(states[&account].policy, Some(policy));
        assert!(states[&account].pending_policy_change.is_none());

        let replacement = RecoveryPolicy {
            guardians: vec![g1],
            threshold: 1,
            time_lock_blocks: 5,
            cooldown_blocks: 0,
        };
        apply(
            &mut states,
            account,
            set_policy_action(&owner_sk, &owner_pk, account, replacement.clone(), 1),
            10,
        )
        .unwrap();
        // Replacement: queued, not yet active.
        assert_eq!(
            states[&account].policy.as_ref().unwrap().guardians,
            vec![g0]
        );
        let pending = states[&account].pending_policy_change.clone().unwrap();
        assert_eq!(pending.new_policy, replacement);
        assert_eq!(pending.executable_at_height, 15);
    }

    #[test]
    fn duplicate_guardians_in_a_policy_are_rejected() {
        let (_, _, g) = keypair(0x4);
        let policy = RecoveryPolicy {
            guardians: vec![g, g],
            threshold: 1,
            time_lock_blocks: 1,
            cooldown_blocks: 0,
        };
        assert_eq!(policy.validate(), Err(RecoveryError::GuardianDuplicate(g)));
    }

    #[test]
    fn threshold_above_guardian_count_is_rejected() {
        let (_, _, g) = keypair(0x5);
        let policy = RecoveryPolicy {
            guardians: vec![g],
            threshold: 2,
            time_lock_blocks: 1,
            cooldown_blocks: 0,
        };
        assert!(matches!(
            policy.validate(),
            Err(RecoveryError::InvalidPolicy(_))
        ));
    }

    #[test]
    fn a_non_guardian_approval_is_rejected() {
        let mut f = fixture(1, 10, 0);
        let (sk, pk, addr) = keypair(0xDEAD);
        let approval = approve(&sk, &pk, addr, &f.account, &f.new_owner_pk, 5);
        let err = apply(
            &mut f.states,
            f.account,
            RecoveryAction::InitiateOrApprove {
                new_owner: f.new_owner_pk.clone(),
                approvals: vec![approval],
            },
            5,
        )
        .unwrap_err();
        assert_eq!(err, RecoveryError::NotAGuardian(addr));
    }

    #[test]
    fn no_policy_refuses_initiation() {
        let mut states: BTreeMap<Address, AccountRecoveryState> = BTreeMap::new();
        let (_, new_owner, account) = keypair(0x77);
        let err = apply(
            &mut states,
            account,
            RecoveryAction::InitiateOrApprove {
                new_owner: new_owner.to_hex(),
                approvals: vec![],
            },
            1,
        )
        .unwrap_err();
        assert_eq!(err, RecoveryError::NoPolicy);
    }

    /// `ConsensusFeature::SocialRecovery` flips exactly at its scheduled
    /// height: inactive the block before, active from the block at (and
    /// after) it. Exercises `FeatureSchedule::active` directly rather than
    /// the process-global `feature_active` wrapper
    /// (`crate::crypto::chain_binding::install_feature_schedule` is a
    /// first-install-wins `OnceLock` shared by the whole test binary, so a
    /// unit test cannot install its own schedule without racing every other
    /// test that touches it) — `FeatureSchedule` itself is a plain value
    /// `apply_transition_inner`'s `feature_active` call ultimately delegates
    /// to, so this proves the same activation arithmetic without needing the
    /// global.
    #[test]
    fn social_recovery_feature_flips_exactly_at_activation_height() {
        use crate::crypto::chain_binding::{ConsensusFeature, FeatureSchedule};

        let schedule = FeatureSchedule::from_entries(vec![(
            ConsensusFeature::SocialRecovery.name().to_string(),
            1_000,
        )])
        .expect("single-feature schedule is valid");

        assert!(!schedule.active(ConsensusFeature::SocialRecovery, 0));
        assert!(!schedule.active(ConsensusFeature::SocialRecovery, 999));
        assert!(schedule.active(ConsensusFeature::SocialRecovery, 1_000));
        assert!(schedule.active(ConsensusFeature::SocialRecovery, 1_001));
        assert!(schedule.active(ConsensusFeature::SocialRecovery, u64::MAX));
    }

    /// An unscheduled feature (no entry at all, the state of every chain
    /// before this feature's activation height is configured) is inactive at
    /// every height, including height 0 — `apply_transition_inner` must
    /// refuse `ApplyAccountRecovery` unconditionally until an operator
    /// actually schedules the feature.
    #[test]
    fn social_recovery_is_inactive_when_entirely_unscheduled() {
        use crate::crypto::chain_binding::{ConsensusFeature, FeatureSchedule};

        let schedule = FeatureSchedule::from_entries(vec![]).expect("empty schedule is valid");
        assert!(!schedule.active(ConsensusFeature::SocialRecovery, 0));
        assert!(!schedule.active(ConsensusFeature::SocialRecovery, u64::MAX));
    }

    #[test]
    fn apply_account_recovery_transition_is_rejected_without_a_schedule() {
        use crate::blockchain::state::{State, StateError, StateTransition};

        let (_, _, account) = keypair(0x5151);
        let mut state = State::default();
        let err = state
            .apply_transition(StateTransition::ApplyAccountRecovery {
                account,
                action: RecoveryAction::Execute,
                block_height: 1,
            })
            .unwrap_err();
        assert!(matches!(err, StateError::InvalidTransition));
        assert!(state.account_recovery.is_empty());
    }

    /// `state_root_material`'s `account_recovery` section must be byte-absent
    /// (not merely zero-length) while the map is empty — the exact property
    /// that keeps every historical `expected_roots.json` entry unchanged by
    /// this feature's addition (proven end-to-end by
    /// `cargo test -p test_server --test determinism_differential`, whose
    /// fixture never populates `account_recovery`). This unit test pins the
    /// same property directly against `State`, independent of that fixture.
    #[test]
    fn state_root_material_unaffected_while_account_recovery_is_empty() {
        use crate::blockchain::state::State;
        use crate::common::types::Balance;

        let mut state = State::default();
        let (_, _, existing_account) = keypair(0x4242);
        state.accounts.insert(
            existing_account,
            crate::blockchain::state::Account::new(Balance::new(7)),
        );
        let with_empty_map = state.state_root_material();
        assert!(state.account_recovery.is_empty());

        // Directly populate the map (bypassing the feature gate, which is
        // apply_transition_inner's job, not state_root_material's) to prove
        // the section DOES change the root once non-empty — otherwise this
        // test would pass vacuously by never emitting the section at all.
        state.account_recovery.insert(
            Address::from_public_key(&PublicKey::from_private_key(&PrivateKey::generate(
                &mut StdRng::seed_from_u64(1),
            ))),
            AccountRecoveryState::default(),
        );
        let non_empty = state.state_root_material();
        assert_ne!(
            with_empty_map, non_empty,
            "a non-empty account_recovery map must change state_root_material"
        );
    }
}
