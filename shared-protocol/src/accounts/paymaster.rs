//! Paymaster / gas sponsorship with its own abuse bounds (#2171).
//!
//! A paymaster lets a third party pay fees for a user operation — the other
//! half of frictionless onboarding (a data buyer paying in a stablecoin, a
//! weather station that should never need to acquire WRF before its first
//! submission). Sponsorship removes the fee-based spam deterrent, so this
//! module exists entirely to put a different deterrent in its place.
//!
//! ## The critical property: everything fails closed, in STATE APPLY
//!
//! Every ceiling here — the deposit, the per-paymaster rate limit, the
//! per-sender rate limit, the post-validation failure-rate throttle — is
//! checked and enforced by [`apply`] against **committed consensus state**,
//! never trusted from the transaction that claims to have already checked
//! it. A sponsor cannot be made to pay beyond its declared bound: an
//! exhausted or absent budget **rejects**, it never silently free-rides on
//! the network. This is what makes sponsorship safe to reopen without
//! reopening the spam hole the economic-abuse epic closed.
//!
//! ## Design decisions and why
//!
//! - **Stake, not a self-declared budget.** A paymaster must post a real
//!   deposit (debited from its own account balance by the caller — see
//!   `StateTransition::ApplyPaymasterAction` in `blockchain/state.rs`) with
//!   an unstake delay. [`PaymasterAccount::deposit`] is what
//!   [`PaymasterAction::SponsorOperation`] actually draws down; there is no
//!   path that spends value the paymaster hasn't already posted.
//! - **Rate limits are a fixed protocol parameter table, not
//!   self-declared.** [`PaymasterLimits`] is not a per-paymaster field a
//!   paymaster could set generously for itself — it is one set of
//!   protocol-wide constants ([`DEFAULT_PAYMASTER_LIMITS`]). The issue's own
//!   text says these should be "sourced from the economic abuse model's
//!   parameter table"; no such table exists yet as a concrete, referenceable
//!   construct in this codebase (verified by search, not assumed) — these
//!   are placeholder protocol defaults pending that table, kept in exactly
//!   one place so replacing them later is a one-line change, not a redesign.
//! - **Fixed-epoch counters, not a sliding window.** Simpler and
//!   deterministic across producer/applier/replay: `epoch_started_at` plus a
//!   count, reset when `height >= epoch_started_at + epoch_blocks`. Good
//!   enough for a rate *ceiling*; a sliding window buys smoothness this
//!   story doesn't ask for.
//! - **Throttle is a consequence of THIS paymaster's own recent record**,
//!   never of a single failure: it fires only once at least
//!   `min_samples_before_throttle` sponsored operations have been observed
//!   in the current epoch AND the failure share of those meets
//!   [`PaymasterLimits::failure_rate_threshold_percent`] — a small sample
//!   of bad luck cannot throttle a paymaster that would otherwise pass.
//! - **A paymaster that has requested unstaking can no longer sponsor.**
//!   Winding down and continuing to draw down the same deposit are
//!   contradictory intents; refusing new sponsorship once
//!   [`PaymasterAccount::unstake_requested_at`] is set keeps the deposit
//!   available for the delay window as an honest safety margin, not a
//!   number that can still shrink while the paymaster is leaving.
//! - **No embedded signature verification in this module.** Every action
//!   here is an action an account takes about its OWN paymaster record
//!   (`Stake`/`RequestUnstake`/`WithdrawStake`) or a fact the surrounding
//!   block-producing/applying pipeline has already established
//!   (`SponsorOperation`'s `succeeded` outcome) — the same shape as the
//!   existing `LockToStake`/`ReleaseToSpendable`/`ApplyReward`
//!   `StateTransition` variants, none of which embed a signature either;
//!   authorization comes from the surrounding transaction, not a
//!   third-party-delegated credential the way #2172's guardian approvals or
//!   #2170's session-key uses require.
//!
//! ## Supply-conservation identity
//!
//! [`apply`] never moves real account balance itself — it returns a
//! [`PaymasterEffect`] describing what the caller must additionally do
//! (`credit_to_account` on a completed withdrawal, `burn` on a sponsored
//! spend), exactly mirroring how `CreateGovernanceProposal`'s deposit is
//! debited from the proposer's account by the `StateTransition` match arm
//! itself, not by a sub-module. A sponsored operation's `actual_cost` is
//! treated as burned (credited to `State::supply_authorized_burning`, the
//! same counter and pattern the existing slashing-burn path already uses)
//! rather than paid to any party — the simplest model that keeps the
//! identity honest without inventing a new fee destination this PR has no
//! mandate to design; a real fee-destination policy, if this codebase
//! settles on one that differs, is a follow-up, not a blocker for the
//! abuse-bounds this story is actually about.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::common::types::Address;

/// Protocol-wide sponsorship rate/throttle parameters. See the module docs
/// for why this is a fixed table, not a per-paymaster field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymasterLimits {
    /// Width, in blocks, of one rate-limit window.
    pub epoch_blocks: u64,
    /// Maximum sponsored operations one paymaster may fund per epoch.
    pub max_sponsorships_per_epoch: u32,
    /// Maximum sponsored operations one paymaster may fund for a single
    /// sender per epoch.
    pub max_sponsorships_per_sender_per_epoch: u32,
    /// Failure-rate percentage (0-100) that trips the throttle, once
    /// `min_samples_before_throttle` is met.
    pub failure_rate_threshold_percent: u8,
    /// Minimum sponsored operations observed in the current epoch before the
    /// failure-rate throttle can fire at all — protects a paymaster with a
    /// tiny sample from being throttled by one or two unlucky failures.
    pub min_samples_before_throttle: u32,
    /// How long, in blocks, a throttle lasts once triggered.
    pub throttle_duration_blocks: u64,
}

/// Placeholder protocol defaults — see the module-level doc for why these
/// are not sourced from an existing "economic abuse model parameter table"
/// (none exists yet as a concrete construct in this codebase).
pub const DEFAULT_PAYMASTER_LIMITS: PaymasterLimits = PaymasterLimits {
    epoch_blocks: 7_200,
    max_sponsorships_per_epoch: 10_000,
    max_sponsorships_per_sender_per_epoch: 200,
    failure_rate_threshold_percent: 20,
    min_samples_before_throttle: 20,
    throttle_duration_blocks: 7_200,
};

/// Per-sender rate-limit bookkeeping inside one paymaster's record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderWindow {
    pub epoch_started_at: u64,
    pub count: u32,
}

/// Consensus state for one paymaster. Keyed in `State::paymasters` by the
/// paymaster's own `Address`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymasterAccount {
    /// Real value posted as stake, debited from the paymaster's own account
    /// balance by the caller when this increases. This is what
    /// [`PaymasterAction::SponsorOperation`] draws down.
    pub deposit: u128,
    pub unstake_delay_blocks: u64,
    /// Height unstaking was requested, if any. Set → no further
    /// `SponsorOperation` is accepted for this paymaster (see module docs).
    pub unstake_requested_at: Option<u64>,
    pub epoch_started_at: u64,
    pub sponsorships_this_epoch: u32,
    pub successes_this_epoch: u32,
    pub failures_this_epoch: u32,
    /// Height the current throttle lasts until, if throttled.
    pub throttled_until: Option<u64>,
    pub per_sender: BTreeMap<Address, SenderWindow>,
}

/// What a single [`apply`] call asks the state machine to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymasterAction {
    /// Post (or add to) a stake. `amount` must already have been debited
    /// from the paymaster's own account balance by the caller before this
    /// is applied — see `StateTransition::ApplyPaymasterAction`.
    /// `unstake_delay_blocks` is fixed at first stake; a later `Stake` with
    /// a different value is refused rather than silently accepted, so a
    /// paymaster cannot shorten its own exit window after building trust.
    Stake {
        paymaster: Address,
        amount: u128,
        unstake_delay_blocks: u64,
    },
    /// Begin the unstake countdown for the paymaster's full current
    /// deposit. Refuses further sponsorship from this moment (see module
    /// docs).
    RequestUnstake { paymaster: Address },
    /// Complete a previously-requested unstake once its delay has elapsed.
    /// Returns the deposit to the caller via [`PaymasterEffect::credit_to_account`].
    WithdrawStake { paymaster: Address },
    /// Debit `actual_cost` from `paymaster`'s deposit for one sponsored
    /// operation on behalf of `sender`. `succeeded` is the sponsored
    /// operation's own outcome (established by whatever executed it,
    /// outside this module's scope) and feeds the failure-rate throttle.
    SponsorOperation {
        paymaster: Address,
        sender: Address,
        actual_cost: u128,
        succeeded: bool,
    },
}

/// What the caller (`StateTransition::ApplyPaymasterAction`'s match arm)
/// must additionally do to a real account or the supply ledger after a
/// successful [`apply`]. See the module-level "Supply-conservation
/// identity" section for why this module never touches either directly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaymasterEffect {
    /// A completed withdrawal: credit this amount back to the paymaster's
    /// account balance.
    pub credit_to_account: Option<u128>,
    /// A sponsored spend: add this amount to `State::supply_authorized_burning`.
    pub burn: Option<u128>,
}

/// Every way [`apply`] refuses an action. `Debug`+`PartialEq`, no panic
/// anywhere in this module.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PaymasterError {
    #[error("no paymaster is registered at this address")]
    NotRegistered,
    #[error("unstake delay {provided} does not match the delay {expected} fixed at first stake")]
    UnstakeDelayMismatch { expected: u64, provided: u64 },
    #[error("unstake already requested at height {requested_at}")]
    UnstakeAlreadyRequested { requested_at: u64 },
    #[error("no unstake has been requested for this paymaster")]
    NoUnstakeRequested,
    #[error("unstake delay has not elapsed: withdrawable at height {withdrawable_at}, currently {height}")]
    UnstakeDelayNotElapsed { withdrawable_at: u64, height: u64 },
    #[error("paymaster has requested unstaking and can no longer sponsor operations")]
    UnstakePending,
    #[error("paymaster is throttled until height {until}")]
    PaymasterThrottled { until: u64 },
    #[error("paymaster has exceeded its per-epoch sponsorship rate limit")]
    PerPaymasterRateLimitExceeded,
    #[error("paymaster has exceeded its per-sender-per-epoch sponsorship rate limit")]
    PerSenderRateLimitExceeded,
    #[error("insufficient deposit: needed {needed}, available {available}")]
    InsufficientDeposit { needed: u128, available: u128 },
    /// #3170: a sponsorship must charge the paymaster something. A zero cost
    /// would consume a rate-limit slot without paying for the work.
    #[error("sponsored cost must be non-zero")]
    ZeroCost,
}

/// Roll `account`'s per-paymaster epoch counters over if the current one has
/// elapsed. A no-op otherwise. Must run before any rate-limit check.
fn roll_epoch_if_elapsed(account: &mut PaymasterAccount, limits: &PaymasterLimits, height: u64) {
    if height >= account.epoch_started_at.saturating_add(limits.epoch_blocks) {
        account.epoch_started_at = height;
        account.sponsorships_this_epoch = 0;
        account.successes_this_epoch = 0;
        account.failures_this_epoch = 0;
        // #3170: per-sender windows live only inside the paymaster's own
        // epoch, so the map is cleared when that epoch rolls. It can then
        // never hold more than `max_sponsorships_per_epoch` entries.
        account.per_sender.clear();
    }
}

/// Roll one sender's window over if the current one has elapsed.
fn roll_sender_window_if_elapsed(window: &mut SenderWindow, limits: &PaymasterLimits, height: u64) {
    if height >= window.epoch_started_at.saturating_add(limits.epoch_blocks) {
        window.epoch_started_at = height;
        window.count = 0;
    }
}

/// Apply one [`PaymasterAction`] against `paymasters` at `height`. The
/// single entry point consensus code calls — see
/// `StateTransition::ApplyPaymasterAction` in `blockchain/state.rs`, whose
/// match arm calls this and then performs whatever [`PaymasterEffect`] it
/// returns against real account balances / the supply ledger.
pub fn apply(
    paymasters: &mut BTreeMap<Address, PaymasterAccount>,
    action: PaymasterAction,
    height: u64,
) -> Result<PaymasterEffect, PaymasterError> {
    match action {
        PaymasterAction::Stake { paymaster, amount, unstake_delay_blocks } => {
            match paymasters.get_mut(&paymaster) {
                Some(existing) => {
                    if existing.unstake_delay_blocks != unstake_delay_blocks {
                        return Err(PaymasterError::UnstakeDelayMismatch {
                            expected: existing.unstake_delay_blocks,
                            provided: unstake_delay_blocks,
                        });
                    }
                    existing.deposit = existing.deposit.saturating_add(amount);
                },
                None => {
                    paymasters.insert(
                        paymaster,
                        PaymasterAccount {
                            deposit: amount,
                            unstake_delay_blocks,
                            unstake_requested_at: None,
                            epoch_started_at: height,
                            sponsorships_this_epoch: 0,
                            successes_this_epoch: 0,
                            failures_this_epoch: 0,
                            throttled_until: None,
                            per_sender: BTreeMap::new(),
                        },
                    );
                },
            }
            Ok(PaymasterEffect::default())
        },
        PaymasterAction::RequestUnstake { paymaster } => {
            let account = paymasters.get_mut(&paymaster).ok_or(PaymasterError::NotRegistered)?;
            if let Some(requested_at) = account.unstake_requested_at {
                return Err(PaymasterError::UnstakeAlreadyRequested { requested_at });
            }
            account.unstake_requested_at = Some(height);
            Ok(PaymasterEffect::default())
        },
        PaymasterAction::WithdrawStake { paymaster } => {
            let account = paymasters.get_mut(&paymaster).ok_or(PaymasterError::NotRegistered)?;
            let requested_at = account.unstake_requested_at.ok_or(PaymasterError::NoUnstakeRequested)?;
            let withdrawable_at = requested_at.saturating_add(account.unstake_delay_blocks);
            if height < withdrawable_at {
                return Err(PaymasterError::UnstakeDelayNotElapsed { withdrawable_at, height });
            }
            let amount = account.deposit;
            account.deposit = 0;
            account.unstake_requested_at = None;
            Ok(PaymasterEffect { credit_to_account: Some(amount), burn: None })
        },
        PaymasterAction::SponsorOperation { paymaster, sender, actual_cost, succeeded } => {
            let limits = DEFAULT_PAYMASTER_LIMITS;
            if actual_cost == 0 {
                return Err(PaymasterError::ZeroCost);
            }
            let account = paymasters.get_mut(&paymaster).ok_or(PaymasterError::NotRegistered)?;
            if account.unstake_requested_at.is_some() {
                return Err(PaymasterError::UnstakePending);
            }
            if let Some(until) = account.throttled_until {
                if height < until {
                    return Err(PaymasterError::PaymasterThrottled { until });
                }
            }
            roll_epoch_if_elapsed(account, &limits, height);
            if account.sponsorships_this_epoch >= limits.max_sponsorships_per_epoch {
                return Err(PaymasterError::PerPaymasterRateLimitExceeded);
            }
            let sender_window = account.per_sender.entry(sender).or_default();
            roll_sender_window_if_elapsed(sender_window, &limits, height);
            if sender_window.count >= limits.max_sponsorships_per_sender_per_epoch {
                return Err(PaymasterError::PerSenderRateLimitExceeded);
            }
            if account.deposit < actual_cost {
                return Err(PaymasterError::InsufficientDeposit {
                    needed: actual_cost,
                    available: account.deposit,
                });
            }

            account.deposit -= actual_cost;
            account.sponsorships_this_epoch += 1;
            account.per_sender.get_mut(&sender).expect("just inserted or already present").count += 1;
            if succeeded {
                account.successes_this_epoch += 1;
            } else {
                account.failures_this_epoch += 1;
            }

            let total = account.successes_this_epoch + account.failures_this_epoch;
            if total >= limits.min_samples_before_throttle {
                let failure_percent = account.failures_this_epoch
                    .saturating_mul(100)
                    / total.max(1);
                if failure_percent >= u32::from(limits.failure_rate_threshold_percent) {
                    account.throttled_until = Some(height.saturating_add(limits.throttle_duration_blocks));
                }
            }

            Ok(PaymasterEffect { credit_to_account: None, burn: Some(actual_cost) })
        },
    }
}

// ---------------------------------------------------------------------------
// Sponsored first sends (owner GO 2026-11-01)
// ---------------------------------------------------------------------------
//
// A sponsor pays the fee for someone else's transparent transfer. The sponsor
// signs a [`SponsorAuthorization`] bound to that exact transfer
// (`from`/`to`/`amount`/`nonce`) and to its own rules (fee, expiry, a balance
// floor it keeps, first-send-only). The transfer carries it in
// `TransparentTransferPayload::sponsorship`. Once `ConsensusFeature::Paymaster`
// is active, state apply checks it, debits the fee from the sponsor's own
// spendable balance, burns it, and records the sponsorship in the sponsor's
// `State::paymasters` entry so the protocol-wide rate limits
// ([`DEFAULT_PAYMASTER_LIMITS`]) bound it. Before activation the field is
// ignored, so a transfer carrying it applies exactly as it does today.
//
// Why this closes the #3169 drain for the reachable path: the only way a
// sponsorship is charged is a valid signature by the sponsor's own key over
// the transfer it pays for, under a chain-bound domain tag, inside a short
// validity window. Nobody else can spend a sponsor's balance. The deposit
// lifecycle actions (`Stake`/`RequestUnstake`/`WithdrawStake`) stay
// unreachable: no transaction constructs them, and they still need owner
// authorization before one does.
//
// Why the sponsor's balance and not a staked deposit: a deposit needs a
// lifecycle transaction (a new `TransactionType`) and an unstake protocol.
// Every rule a sponsor sets is already in its own signature, and its exposure
// is bounded on chain by the fee cap times the per-epoch count. The balance
// floor lets the sponsor stop paying before its account runs dry.

/// Domain tag for a sponsor's authorization of one transfer's fee.
pub const SPONSORED_SEND_AUTH_DOMAIN: &[u8] = b"wrfcoin_sponsored_send_auth_v1";

/// Smallest fee a sponsorship may pay, in base units (0.00001 WRF). #3170: a
/// sponsorship always costs the sponsor something.
pub const MIN_SPONSORED_FEE: u64 = 1_000;

/// Largest fee a sponsorship may pay, in base units (0.01 WRF). Caps what a
/// leaked sponsor key can burn per transfer.
pub const MAX_SPONSORED_FEE: u64 = 1_000_000;

/// Longest a sponsor authorization may stay valid, in blocks (about 41
/// minutes at 77 s/block). A sponsor cannot be made to pay for a transfer it
/// signed long ago; a stockpile of signatures expires.
pub const MAX_SPONSORSHIP_VALIDITY_BLOCKS: u64 = 32;

/// A sponsor's signed promise to pay one transfer's fee, carried in
/// `TransparentTransferPayload::sponsorship`. Hex/JSON wire form so wallets
/// and the sponsor service can build it without Rust.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SponsorAuthorization {
    /// Sponsor account address (hex). Its spendable balance pays `fee`.
    pub sponsor: String,
    /// Fee the sponsor pays, in base units. Burned.
    pub fee: u64,
    /// Last height at which this authorization may be applied.
    pub expires_at_height: u64,
    /// The sponsor refuses to pay if its spendable balance would drop below
    /// this, in base units.
    pub balance_floor: u64,
    /// Pay only when the transfer is the sender's first outgoing transfer
    /// (account nonce 0).
    pub first_send_only: bool,
    /// Hex ed25519 public key of the sponsor; must derive `sponsor`.
    pub sponsor_public_key: String,
    /// Hex ed25519 signature over [`sponsored_send_authorization_message_with_domain`].
    pub signature: String,
}

/// Why a sponsorship is refused. Each refusal leaves state untouched.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SponsorshipError {
    #[error("sponsor address is not a valid address")]
    SponsorAddressInvalid,
    #[error("sponsor public key or signature is malformed")]
    MalformedSignature,
    #[error("sponsor public key does not derive the sponsor address")]
    SponsorKeyMismatch,
    #[error("sponsor signature does not verify over this transfer")]
    BadSignature,
    #[error("an account cannot sponsor its own transfer")]
    SelfSponsorship,
    #[error("sponsorship expired at height {expires_at_height}, now {height}")]
    Expired { expires_at_height: u64, height: u64 },
    #[error("sponsorship is valid for more than {max} blocks")]
    ValidityTooLong { max: u64 },
    #[error("sponsored fee {fee} is below the protocol minimum {min}")]
    FeeBelowMinimum { fee: u64, min: u64 },
    #[error("sponsored fee {fee} is above the protocol cap {max}")]
    FeeAboveCap { fee: u64, max: u64 },
    #[error("sponsor pays only for a first send; transfer nonce is {nonce}")]
    NotFirstSend { nonce: u64 },
    #[error("sponsor balance {available} cannot pay {fee} and keep its floor {floor}")]
    SponsorBalanceBelowFloor { available: u128, fee: u64, floor: u64 },
    #[error("sponsor rate limit: {0}")]
    RateLimited(PaymasterError),
}

/// The message a sponsor signs, with the chain identity passed explicitly.
/// Every variable-length field is length-prefixed; the trailing fields are
/// fixed width, so no two distinct sponsorships share a preimage.
#[allow(clippy::too_many_arguments)]
pub fn sponsored_send_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    sponsor: &str,
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
    fee: u64,
    expires_at_height: u64,
    balance_floor: u64,
    first_send_only: bool,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(SPONSORED_SEND_AUTH_DOMAIN.len() + 160);
    buf.extend_from_slice(SPONSORED_SEND_AUTH_DOMAIN);
    crate::blockchain::transaction::push_chain_domain(&mut buf, chain_id, network_id);
    for field in [sponsor, from, to] {
        buf.extend_from_slice(&(field.len() as u32).to_le_bytes());
        buf.extend_from_slice(field.as_bytes());
    }
    buf.extend_from_slice(&amount.to_le_bytes());
    buf.extend_from_slice(&nonce.to_le_bytes());
    buf.extend_from_slice(&fee.to_le_bytes());
    buf.extend_from_slice(&expires_at_height.to_le_bytes());
    buf.extend_from_slice(&balance_floor.to_le_bytes());
    buf.push(u8::from(first_send_only));
    buf
}

impl SponsorAuthorization {
    /// The message this authorization signs for the transfer
    /// `from -> to, amount` at `nonce`, under the running chain identity.
    pub fn message(&self, from: &str, to: &str, amount: u64, nonce: u64) -> Vec<u8> {
        sponsored_send_authorization_message_with_domain(
            &crate::crypto::domain_signing::signing_chain_id(),
            &crate::crypto::domain_signing::signing_network_id(),
            &self.sponsor,
            from,
            to,
            amount,
            nonce,
            self.fee,
            self.expires_at_height,
            self.balance_floor,
            self.first_send_only,
        )
    }

    /// Fill `sponsor_public_key`/`signature` with `key`'s signature over this
    /// transfer. The sponsor service and tests use it.
    pub fn sign(
        &mut self,
        key: &crate::crypto::signature::PrivateKey,
        from: &str,
        to: &str,
        amount: u64,
        nonce: u64,
    ) {
        let message = self.message(from, to, amount, nonce);
        self.sponsor_public_key =
            crate::crypto::signature::PublicKey::from_private_key(key).to_hex();
        self.signature = key.sign(&message).to_hex();
    }

    /// The signature alone: well-formed, from the key that derives
    /// `sponsor`, over this transfer. Height-free, so mempool admission can
    /// use it. Returns the sponsor address.
    pub fn verify_signature(
        &self,
        from: &str,
        to: &str,
        amount: u64,
        nonce: u64,
    ) -> Result<Address, SponsorshipError> {
        let sponsor = self
            .sponsor
            .parse::<Address>()
            .map_err(|_| SponsorshipError::SponsorAddressInvalid)?;
        let public_key = crate::crypto::signature::PublicKey::from_hex(&self.sponsor_public_key)
            .map_err(|_| SponsorshipError::MalformedSignature)?;
        let signature = crate::crypto::signature::Signature::from_hex(&self.signature)
            .map_err(|_| SponsorshipError::MalformedSignature)?;
        if Address::from_public_key(&public_key) != sponsor {
            return Err(SponsorshipError::SponsorKeyMismatch);
        }
        if !public_key.verify(&self.message(from, to, amount, nonce), &signature) {
            return Err(SponsorshipError::BadSignature);
        }
        Ok(sponsor)
    }

    /// Every stateless rule at `height`: the signature, no self-sponsorship,
    /// the validity window, the fee bounds and first-send-only. Returns the
    /// sponsor address. Balance and rate limits need state; see
    /// [`charge_balance_sponsorship`].
    pub fn check(
        &self,
        from: &str,
        to: &str,
        amount: u64,
        nonce: u64,
        height: u64,
    ) -> Result<Address, SponsorshipError> {
        let sponsor = self.verify_signature(from, to, amount, nonce)?;
        if from.parse::<Address>().ok() == Some(sponsor) {
            return Err(SponsorshipError::SelfSponsorship);
        }
        if height > self.expires_at_height {
            return Err(SponsorshipError::Expired {
                expires_at_height: self.expires_at_height,
                height,
            });
        }
        if self.expires_at_height - height > MAX_SPONSORSHIP_VALIDITY_BLOCKS {
            return Err(SponsorshipError::ValidityTooLong {
                max: MAX_SPONSORSHIP_VALIDITY_BLOCKS,
            });
        }
        if self.fee < MIN_SPONSORED_FEE {
            return Err(SponsorshipError::FeeBelowMinimum {
                fee: self.fee,
                min: MIN_SPONSORED_FEE,
            });
        }
        if self.fee > MAX_SPONSORED_FEE {
            return Err(SponsorshipError::FeeAboveCap {
                fee: self.fee,
                max: MAX_SPONSORED_FEE,
            });
        }
        if self.first_send_only && nonce != 0 {
            return Err(SponsorshipError::NotFirstSend { nonce });
        }
        Ok(sponsor)
    }
}

/// Lenient deserializer for `TransparentTransferPayload::sponsorship`: a
/// malformed member reads as `None`, so a payload an older binary parses is
/// parsed identically here (same rule as `passkey::deserialize_lenient_passkey`).
pub fn deserialize_lenient_sponsorship<'de, D>(
    deserializer: D,
) -> Result<Option<SponsorAuthorization>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw.and_then(|value| serde_json::from_value::<SponsorAuthorization>(value).ok()))
}

/// Whether `ConsensusFeature::Paymaster` is in force at `height`.
pub fn sponsorship_active(height: u64) -> bool {
    crate::crypto::chain_binding::feature_active(
        crate::crypto::chain_binding::ConsensusFeature::Paymaster,
        height,
    )
}

/// Whether this chain schedules `ConsensusFeature::Paymaster` at all
/// (mempool admission has no height).
pub fn sponsorship_scheduled() -> bool {
    crate::crypto::chain_binding::feature_scheduled(
        crate::crypto::chain_binding::ConsensusFeature::Paymaster,
    )
}

/// Rate-limit one balance-funded sponsorship of `cost` for `sender` against
/// the sponsor's record, WITHOUT mutating `paymasters`: returns the updated
/// record for the caller to store once the transfer itself has applied. A
/// sponsor with no record gets one (deposit 0). The deposit is never read or
/// drawn: the caller debits the sponsor's spendable balance.
pub fn charge_balance_sponsorship(
    paymasters: &BTreeMap<Address, PaymasterAccount>,
    sponsor: Address,
    sender: Address,
    cost: u128,
    height: u64,
) -> Result<PaymasterAccount, PaymasterError> {
    let limits = DEFAULT_PAYMASTER_LIMITS;
    if cost == 0 {
        return Err(PaymasterError::ZeroCost);
    }
    let mut account = paymasters.get(&sponsor).cloned().unwrap_or(PaymasterAccount {
        deposit: 0,
        unstake_delay_blocks: 0,
        unstake_requested_at: None,
        epoch_started_at: height,
        sponsorships_this_epoch: 0,
        successes_this_epoch: 0,
        failures_this_epoch: 0,
        throttled_until: None,
        per_sender: BTreeMap::new(),
    });
    if account.unstake_requested_at.is_some() {
        return Err(PaymasterError::UnstakePending);
    }
    if let Some(until) = account.throttled_until {
        if height < until {
            return Err(PaymasterError::PaymasterThrottled { until });
        }
    }
    roll_epoch_if_elapsed(&mut account, &limits, height);
    if account.sponsorships_this_epoch >= limits.max_sponsorships_per_epoch {
        return Err(PaymasterError::PerPaymasterRateLimitExceeded);
    }
    let window = account.per_sender.entry(sender).or_default();
    roll_sender_window_if_elapsed(window, &limits, height);
    if window.count >= limits.max_sponsorships_per_sender_per_epoch {
        return Err(PaymasterError::PerSenderRateLimitExceeded);
    }
    window.count += 1;
    account.sponsorships_this_epoch += 1;
    // A balance sponsorship is recorded only once its transfer applied, so
    // it is always a success; the failure throttle never fires on this path.
    account.successes_this_epoch += 1;
    Ok(account)
}

#[cfg(test)]
mod sponsored_send_tests {
    use super::*;
    use crate::crypto::signature::{PrivateKey, PublicKey};

    const FROM: &str = "0x1111111111111111111111111111111111111111";
    const TO: &str = "0x2222222222222222222222222222222222222222";

    fn sponsor_key() -> PrivateKey {
        PrivateKey::from_bytes(&[7u8; 32]).expect("key")
    }

    fn signed(fee: u64, expires_at_height: u64, first_send_only: bool, nonce: u64) -> SponsorAuthorization {
        let key = sponsor_key();
        let sponsor = Address::from_public_key(&PublicKey::from_private_key(&key)).to_hex();
        let mut auth = SponsorAuthorization {
            sponsor,
            fee,
            expires_at_height,
            balance_floor: 0,
            first_send_only,
            sponsor_public_key: String::new(),
            signature: String::new(),
        };
        auth.sign(&key, FROM, TO, 10, nonce);
        auth
    }

    #[test]
    fn a_valid_sponsorship_passes_and_names_its_sponsor() {
        let auth = signed(5_000, 110, true, 0);
        let sponsor = auth.check(FROM, TO, 10, 0, 100).expect("valid");
        assert_eq!(sponsor.to_hex(), auth.sponsor);
    }

    #[test]
    fn the_signature_binds_every_transfer_field() {
        let auth = signed(5_000, 110, true, 0);
        assert_eq!(auth.check(FROM, TO, 11, 0, 100), Err(SponsorshipError::BadSignature));
        assert_eq!(auth.check(FROM, FROM, 10, 0, 100), Err(SponsorshipError::BadSignature));
        let mut raised = auth.clone();
        raised.fee += 1;
        assert_eq!(raised.check(FROM, TO, 10, 0, 100), Err(SponsorshipError::BadSignature));
        let mut floor = auth.clone();
        floor.balance_floor = 1;
        assert_eq!(floor.check(FROM, TO, 10, 0, 100), Err(SponsorshipError::BadSignature));
    }

    #[test]
    fn a_key_that_does_not_derive_the_sponsor_is_refused() {
        let mut auth = signed(5_000, 110, true, 0);
        auth.sponsor = FROM.to_string();
        assert_eq!(auth.check(TO, TO, 10, 0, 100), Err(SponsorshipError::SponsorKeyMismatch));
    }

    #[test]
    fn expiry_and_validity_window_are_enforced() {
        let auth = signed(5_000, 110, true, 0);
        assert!(matches!(auth.check(FROM, TO, 10, 0, 111), Err(SponsorshipError::Expired { .. })));
        assert!(matches!(
            auth.check(FROM, TO, 10, 0, 110 - MAX_SPONSORSHIP_VALIDITY_BLOCKS - 1),
            Err(SponsorshipError::ValidityTooLong { .. })
        ));
        assert!(auth.check(FROM, TO, 10, 0, 110).is_ok());
    }

    #[test]
    fn fee_bounds_are_enforced() {
        let low = signed(MIN_SPONSORED_FEE - 1, 110, true, 0);
        assert!(matches!(low.check(FROM, TO, 10, 0, 100), Err(SponsorshipError::FeeBelowMinimum { .. })));
        let high = signed(MAX_SPONSORED_FEE + 1, 110, true, 0);
        assert!(matches!(high.check(FROM, TO, 10, 0, 100), Err(SponsorshipError::FeeAboveCap { .. })));
    }

    #[test]
    fn first_send_only_refuses_a_later_send() {
        let auth = signed(5_000, 110, true, 3);
        assert_eq!(auth.check(FROM, TO, 10, 3, 100), Err(SponsorshipError::NotFirstSend { nonce: 3 }));
        let any = signed(5_000, 110, false, 3);
        assert!(any.check(FROM, TO, 10, 3, 100).is_ok());
    }

    #[test]
    fn a_sponsor_cannot_sponsor_itself() {
        let key = sponsor_key();
        let me = Address::from_public_key(&PublicKey::from_private_key(&key)).to_hex();
        let mut auth = SponsorAuthorization {
            sponsor: me.clone(),
            fee: 5_000,
            expires_at_height: 110,
            balance_floor: 0,
            first_send_only: false,
            sponsor_public_key: String::new(),
            signature: String::new(),
        };
        auth.sign(&key, &me, TO, 10, 0);
        assert_eq!(auth.check(&me, TO, 10, 0, 100), Err(SponsorshipError::SelfSponsorship));
    }

    #[test]
    fn malformed_sponsorship_json_reads_as_absent() {
        #[derive(Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "deserialize_lenient_sponsorship")]
            sponsorship: Option<SponsorAuthorization>,
        }
        let h: Holder = serde_json::from_str(r#"{"sponsorship":{"sponsor":7}}"#).unwrap();
        assert!(h.sponsorship.is_none());
        let h: Holder = serde_json::from_str("{}").unwrap();
        assert!(h.sponsorship.is_none());
    }

    #[test]
    fn balance_sponsorship_never_mutates_the_map_and_rate_limits() {
        let paymasters = BTreeMap::new();
        let sponsor = Address([9; 20]);
        let sender = Address([1; 20]);
        let record = charge_balance_sponsorship(&paymasters, sponsor, sender, 5_000, 10).unwrap();
        assert!(paymasters.is_empty());
        assert_eq!(record.deposit, 0);
        assert_eq!(record.sponsorships_this_epoch, 1);
        assert_eq!(record.per_sender[&sender].count, 1);
        assert_eq!(
            charge_balance_sponsorship(&paymasters, sponsor, sender, 0, 10),
            Err(PaymasterError::ZeroCost)
        );

        let mut paymasters = BTreeMap::new();
        let mut full = record.clone();
        full.sponsorships_this_epoch = DEFAULT_PAYMASTER_LIMITS.max_sponsorships_per_epoch;
        paymasters.insert(sponsor, full);
        assert_eq!(
            charge_balance_sponsorship(&paymasters, sponsor, Address([2; 20]), 5_000, 11),
            Err(PaymasterError::PerPaymasterRateLimitExceeded)
        );
        // The epoch rolls: counters AND the per-sender map reset (#3170).
        let rolled = charge_balance_sponsorship(
            &paymasters,
            sponsor,
            Address([2; 20]),
            5_000,
            10 + DEFAULT_PAYMASTER_LIMITS.epoch_blocks,
        )
        .unwrap();
        assert_eq!(rolled.sponsorships_this_epoch, 1);
        assert_eq!(rolled.per_sender.len(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    fn staked(deposit: u128, unstake_delay_blocks: u64) -> BTreeMap<Address, PaymasterAccount> {
        let mut paymasters = BTreeMap::new();
        apply(
            &mut paymasters,
            PaymasterAction::Stake { paymaster: addr(1), amount: deposit, unstake_delay_blocks },
            0,
        )
        .unwrap();
        paymasters
    }

    #[test]
    fn sponsored_op_debits_deposit_by_actual_cost() {
        let mut paymasters = staked(1_000, 100);
        let effect = apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: addr(2),
                actual_cost: 40,
                succeeded: true,
            },
            10,
        )
        .unwrap();
        assert_eq!(effect, PaymasterEffect { credit_to_account: None, burn: Some(40) });
        assert_eq!(paymasters[&addr(1)].deposit, 960);
    }

    /// "A zero-balance sender can transact when sponsored": the sponsored
    /// sender's own balance is never read, checked, or required anywhere in
    /// this module — the deposit debited is entirely the PAYMASTER's.
    #[test]
    fn sponsorship_never_touches_or_requires_sender_balance() {
        let mut paymasters = staked(1_000, 100);
        // A sender that this module has no balance concept for at all --
        // there is no Account, no State, nothing but an Address. If
        // sponsorship worked, the sender's (nonexistent) balance was
        // irrelevant, exactly as the acceptance criterion requires.
        let broke_sender = addr(0xFF);
        apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: broke_sender,
                actual_cost: 1,
                succeeded: true,
            },
            10,
        )
        .expect("sponsorship must succeed with zero information about the sender's balance");
    }

    #[test]
    fn insufficient_deposit_rejects_at_validation() {
        let mut paymasters = staked(30, 100);
        let err = apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: addr(2),
                actual_cost: 40,
                succeeded: true,
            },
            10,
        )
        .unwrap_err();
        assert_eq!(err, PaymasterError::InsufficientDeposit { needed: 40, available: 30 });
        // Fail-closed: deposit must be untouched.
        assert_eq!(paymasters[&addr(1)].deposit, 30);
    }

    #[test]
    fn per_paymaster_rate_limit_is_enforced() {
        // `apply` always uses the fixed DEFAULT_PAYMASTER_LIMITS (see module
        // docs on why limits are a protocol table, not per-paymaster), so
        // this drives exactly that many successful sponsorships and asserts
        // the next one is refused.
        let mut paymasters = staked(u128::MAX / 2, 100);
        let cap = DEFAULT_PAYMASTER_LIMITS.max_sponsorships_per_epoch;
        // Fixed height (all within one epoch -- realistic: many operations
        // land in the same or nearby blocks) and rotated senders (well under
        // the per-sender cap each) so the PER-PAYMASTER limit is the only
        // thing this test can hit.
        for i in 0..cap {
            let sender = addr(u8::try_from(2 + (i % 200)).unwrap());
            apply(
                &mut paymasters,
                PaymasterAction::SponsorOperation {
                    paymaster: addr(1),
                    sender,
                    actual_cost: 1,
                    succeeded: true,
                },
                10,
            )
            .expect("within the per-epoch cap must succeed");
        }
        let err = apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: addr(2),
                actual_cost: 1,
                succeeded: true,
            },
            10,
        )
        .unwrap_err();
        assert_eq!(err, PaymasterError::PerPaymasterRateLimitExceeded);
    }

    #[test]
    fn per_sender_rate_limit_is_enforced_independently_of_other_senders() {
        let mut paymasters = staked(u128::MAX / 2, 100);
        let cap = DEFAULT_PAYMASTER_LIMITS.max_sponsorships_per_sender_per_epoch;
        for _ in 0..cap {
            apply(
                &mut paymasters,
                PaymasterAction::SponsorOperation {
                    paymaster: addr(1),
                    sender: addr(2),
                    actual_cost: 1,
                    succeeded: true,
                },
                10,
            )
            .unwrap();
        }
        let err = apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: addr(2),
                actual_cost: 1,
                succeeded: true,
            },
            10,
        )
        .unwrap_err();
        assert_eq!(err, PaymasterError::PerSenderRateLimitExceeded);

        // A DIFFERENT sender is unaffected.
        apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: addr(3),
                actual_cost: 1,
                succeeded: true,
            },
            10,
        )
        .expect("a different sender's own window must be independent");
    }

    #[test]
    fn high_post_validation_failure_rate_triggers_throttling() {
        let mut paymasters = staked(u128::MAX / 2, 100);
        let threshold = DEFAULT_PAYMASTER_LIMITS.failure_rate_threshold_percent;
        let min_samples = DEFAULT_PAYMASTER_LIMITS.min_samples_before_throttle;

        // Drive exactly `min_samples` operations, enough failures to clear
        // the threshold percentage.
        let failures_needed = (u32::from(threshold) * min_samples).div_ceil(100).max(1);
        for i in 0..min_samples {
            let succeeded = i >= failures_needed;
            apply(
                &mut paymasters,
                PaymasterAction::SponsorOperation {
                    paymaster: addr(1),
                    sender: addr(u8::try_from(2 + (i % 50)).unwrap()),
                    actual_cost: 1,
                    succeeded,
                },
                10,
            )
            .unwrap();
        }

        assert!(paymasters[&addr(1)].throttled_until.is_some(), "high failure rate must trigger a throttle");

        let throttled_until = paymasters[&addr(1)].throttled_until.unwrap();
        let err = apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: addr(99),
                actual_cost: 1,
                succeeded: true,
            },
            throttled_until - 1,
        )
        .unwrap_err();
        assert_eq!(err, PaymasterError::PaymasterThrottled { until: throttled_until });

        // Once the throttle window passes, sponsorship resumes (a fresh
        // epoch also resets the failure counters, so this must succeed).
        apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: addr(99),
                actual_cost: 1,
                succeeded: true,
            },
            throttled_until + DEFAULT_PAYMASTER_LIMITS.epoch_blocks,
        )
        .expect("sponsorship must resume once the throttle window has passed");
    }

    #[test]
    fn a_low_failure_rate_does_not_throttle() {
        let mut paymasters = staked(u128::MAX / 2, 100);
        let min_samples = DEFAULT_PAYMASTER_LIMITS.min_samples_before_throttle;
        for i in 0..min_samples {
            apply(
                &mut paymasters,
                PaymasterAction::SponsorOperation {
                    paymaster: addr(1),
                    sender: addr(u8::try_from(2 + (i % 50)).unwrap()),
                    actual_cost: 1,
                    succeeded: true,
                },
                10,
            )
            .unwrap();
        }
        assert!(paymasters[&addr(1)].throttled_until.is_none());
    }

    #[test]
    fn too_few_samples_never_throttles_even_at_100_percent_failure() {
        let mut paymasters = staked(u128::MAX / 2, 100);
        let min_samples = DEFAULT_PAYMASTER_LIMITS.min_samples_before_throttle;
        for i in 0..(min_samples - 1) {
            apply(
                &mut paymasters,
                PaymasterAction::SponsorOperation {
                    paymaster: addr(1),
                    sender: addr(u8::try_from(2 + (i % 50)).unwrap()),
                    actual_cost: 1,
                    succeeded: false,
                },
                10,
            )
            .unwrap();
        }
        assert!(paymasters[&addr(1)].throttled_until.is_none());
    }

    #[test]
    fn unstake_then_withdraw_after_delay_credits_the_full_deposit() {
        let mut paymasters = staked(500, 50);
        apply(&mut paymasters, PaymasterAction::RequestUnstake { paymaster: addr(1) }, 10).unwrap();

        let err = apply(&mut paymasters, PaymasterAction::WithdrawStake { paymaster: addr(1) }, 20).unwrap_err();
        assert_eq!(err, PaymasterError::UnstakeDelayNotElapsed { withdrawable_at: 60, height: 20 });

        let effect = apply(&mut paymasters, PaymasterAction::WithdrawStake { paymaster: addr(1) }, 60).unwrap();
        assert_eq!(effect, PaymasterEffect { credit_to_account: Some(500), burn: None });
        assert_eq!(paymasters[&addr(1)].deposit, 0);
    }

    #[test]
    fn a_paymaster_with_a_pending_unstake_cannot_sponsor() {
        let mut paymasters = staked(500, 50);
        apply(&mut paymasters, PaymasterAction::RequestUnstake { paymaster: addr(1) }, 10).unwrap();
        let err = apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: addr(2),
                actual_cost: 1,
                succeeded: true,
            },
            11,
        )
        .unwrap_err();
        assert_eq!(err, PaymasterError::UnstakePending);
    }

    #[test]
    fn a_mismatched_unstake_delay_on_a_later_stake_is_rejected() {
        let mut paymasters = staked(500, 50);
        let err = apply(
            &mut paymasters,
            PaymasterAction::Stake { paymaster: addr(1), amount: 10, unstake_delay_blocks: 999 },
            20,
        )
        .unwrap_err();
        assert_eq!(err, PaymasterError::UnstakeDelayMismatch { expected: 50, provided: 999 });
        // Rejected -- the original deposit is untouched.
        assert_eq!(paymasters[&addr(1)].deposit, 500);
    }

    #[test]
    fn sponsoring_through_an_unregistered_paymaster_is_rejected() {
        let mut paymasters: BTreeMap<Address, PaymasterAccount> = BTreeMap::new();
        let err = apply(
            &mut paymasters,
            PaymasterAction::SponsorOperation {
                paymaster: addr(1),
                sender: addr(2),
                actual_cost: 1,
                succeeded: true,
            },
            10,
        )
        .unwrap_err();
        assert_eq!(err, PaymasterError::NotRegistered);
    }
}
