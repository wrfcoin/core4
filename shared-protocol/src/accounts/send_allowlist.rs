//! Chain-enforced send allowlist (owner request 2026-11-01).
//!
//! An account can turn on a policy under which consensus refuses every value
//! movement out of the account to an address that is not on the account's
//! list, whatever wallet or app built the transaction. The point is to keep
//! funds safe from someone who has learned the account's passphrase: the
//! thief can sign anything the owner can, so every rule below is about what a
//! signature alone is NOT allowed to do quickly.
//!
//! - **Turning the policy on, removing an address and raising the delay are
//!   immediate.** Each one only lowers what the account can do, so there is
//!   nothing for a thief to gain from them.
//! - **Adding an address waits [`SendAllowlistPolicy::add_delay_blocks`]**
//!   (at least [`MIN_ADD_DELAY_BLOCKS`]). Until then the address is *pending*:
//!   sends to it are refused and the owner, with the account key, can cancel
//!   it. Wallets show pending additions (`GET /accounts/{address}/send-allowlist`)
//!   so the owner notices one they did not ask for.
//! - **Turning the policy off waits the same delay** and is cancellable the
//!   same way. Otherwise "turn it off, then send" would be a one-block bypass.
//! - **The delay can only go up while the policy is on.** Lowering it would
//!   shorten the window a thief has to wait. To lower it, turn the policy off
//!   (time-locked) and on again.
//! - **A cancellation starts a cooldown** ([`CANCEL_COOLDOWN_BLOCKS`]) during
//!   which no new addition or disable may be requested. Without it, a thief
//!   holding the key could re-request the moment the owner cancels and force
//!   the owner to watch the chain continuously.
//! - **Every owner action is signed** over a domain-separated preimage that
//!   binds the chain id, the account, the exact action and the account's
//!   action nonce ([`send_allowlist_action_message`]), so a signature cannot be
//!   replayed onto another chain, another account, another action or a later
//!   moment of the same account.
//!
//! What this does not protect against: a thief who holds the key and is
//! patient. If the owner never looks at pending additions, an address the
//! thief adds becomes effective after the delay. The policy buys the owner
//! the delay, nothing more; the wallet's job is to make a pending addition
//! impossible to miss.
//!
//! ## Why a separate per-account policy, not an `AuthorizationRule`
//!
//! [`crate::accounts::authorization::AuthorizationRule`] answers *who may sign*
//! a debit. This answers *where a debit may go*, and it must hold whatever
//! signed it: the account's ed25519 key, a passkey (#2169), a session key
//! (#2170) or a future delegate. As an authorization rule it would be one
//! alternative among several (an account declares one rule), it would be
//! bypassed by any path that does not go through dispatch, and dispatch
//! itself is still unscheduled. So the allowlist is its own state
//! (`State::send_allowlists`) consulted at the value-moving apply seams, after
//! authorization has already succeeded. See [`value_movement`] for the
//! classification of every transaction type.
//!
//! ## Why block heights
//!
//! Same reason as `recovery.rs`: consensus needs a clock every node agrees on
//! byte for byte. At chain 888's ~77 s per block, [`DEFAULT_ADD_DELAY_BLOCKS`]
//! is about 48 h and [`MIN_ADD_DELAY_BLOCKS`] about 24 h.
//!
//! ## Activation
//!
//! Gated by `ConsensusFeature::SendAllowlist`. Below its activation
//! `StateTransition::ProcessGovernanceTransaction` refuses every
//! `TransactionType::SendAllowlist`, mempool admission refuses it and the
//! block classifier invalidates a block that carries one, so
//! `State::send_allowlists` is provably empty. Every guard reads that map
//! only, so with it empty every value path is byte-identical to a binary
//! without this module, and the map is absent from `state_root_material`,
//! checkpoints and snapshots while empty.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::blockchain::transaction::{Transaction, TransactionType};
use crate::common::types::Address;
use crate::crypto::signature::{PublicKey, Signature};

/// Domain tag for every owner-signed send-allowlist action.
pub const SEND_ALLOWLIST_ACTION_DOMAIN_TAG: &[u8] = b"wrfcoin.send_allowlist_action.v1";

/// Smallest allowed waiting period for additions and for turning the policy
/// off: 1,122 blocks, about 24 h at 77 s per block.
pub const MIN_ADD_DELAY_BLOCKS: u64 = 1_122;
/// Suggested waiting period: 2,244 blocks, about 48 h at 77 s per block.
pub const DEFAULT_ADD_DELAY_BLOCKS: u64 = 2_244;
/// Largest waiting period: 33,662 blocks, about 30 days at 77 s per block.
/// Bounded so a typo cannot lock an account's list for years.
pub const MAX_ADD_DELAY_BLOCKS: u64 = 33_662;
/// After any cancellation, no new addition or disable may be requested for
/// this many blocks: 561 blocks, about 12 h at 77 s per block.
pub const CANCEL_COOLDOWN_BLOCKS: u64 = 561;
/// Most addresses one account's list may hold, counting pending additions.
pub const MAX_ALLOWED_ADDRESSES: usize = 64;
/// Most additions one account may have pending at once.
pub const MAX_PENDING_ADDITIONS: usize = 8;
/// Largest `Transaction::data` this module parses.
pub const MAX_SEND_ALLOWLIST_PAYLOAD_BYTES: usize = 16 * 1024;

/// A time-locked change waiting to take effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingChange {
    pub requested_at_height: u64,
    /// The change is in force from this height on (inclusive).
    pub effective_at_height: u64,
}

/// An account's policy while it is on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendAllowlistPolicy {
    /// Height the policy was turned on.
    pub enabled_at_height: u64,
    /// Waiting period for additions and for turning the policy off.
    pub add_delay_blocks: u64,
    /// Addresses value may be sent to now.
    pub allowed: BTreeSet<Address>,
    /// Additions waiting out the delay, by address.
    pub pending_additions: BTreeMap<Address, PendingChange>,
    /// A requested turn-off waiting out the delay.
    pub pending_disable: Option<PendingChange>,
    /// No new addition or disable may be requested below this height
    /// (set by a cancellation).
    pub requests_blocked_until_height: Option<u64>,
}

impl SendAllowlistPolicy {
    /// Whether `to` may receive value from `account` at `height`. The account
    /// itself always may: a self-send moves nothing out.
    pub fn permits(&self, account: &Address, to: &Address, height: u64) -> bool {
        to == account
            || self.allowed.contains(to)
            || self
                .pending_additions
                .get(to)
                .is_some_and(|pending| height >= pending.effective_at_height)
    }

    fn disable_matured(&self, height: u64) -> bool {
        self.pending_disable
            .is_some_and(|pending| height >= pending.effective_at_height)
    }
}

/// Per-account state. Absent (no entry in `State::send_allowlists`) is the
/// default for every account. The entry is kept after the policy is turned
/// off so that `action_nonce` never resets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSendAllowlist {
    /// Every accepted action consumes the current value; the next action must
    /// carry the incremented one.
    pub action_nonce: u64,
    pub policy: Option<SendAllowlistPolicy>,
}

impl AccountSendAllowlist {
    /// The policy in force at `height`, or `None` when the account is
    /// unrestricted at that height (never enabled, or a disable has matured).
    pub fn policy_in_force(&self, height: u64) -> Option<&SendAllowlistPolicy> {
        self.policy.as_ref().filter(|policy| !policy.disable_matured(height))
    }

    /// Fold matured time-locks into the stored state. Pure function of the
    /// stored state and `height`; called before every action so stored state
    /// never needs a per-block hook.
    fn settle(&mut self, height: u64) {
        let Some(policy) = self.policy.as_mut() else {
            return;
        };
        if policy.disable_matured(height) {
            self.policy = None;
            return;
        }
        let matured: Vec<Address> = policy
            .pending_additions
            .iter()
            .filter(|(_, pending)| height >= pending.effective_at_height)
            .map(|(address, _)| *address)
            .collect();
        for address in matured {
            policy.pending_additions.remove(&address);
            policy.allowed.insert(address);
        }
    }
}

/// One owner action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendAllowlistAction {
    /// Turn the policy on, immediately, with an initial list. Only while it is
    /// off: the initial list is not time-locked because before this action the
    /// account could already send anywhere.
    Enable {
        add_delay_blocks: u64,
        allowed: Vec<Address>,
    },
    /// Ask for `address` to be added. Effective after the policy's delay.
    RequestAdd { address: Address },
    /// Cancel a pending addition. Starts the cooldown.
    CancelAdd { address: Address },
    /// Remove an address (in force or pending). Immediate.
    Remove { address: Address },
    /// Raise the delay. Immediate; only upward.
    RaiseDelay { add_delay_blocks: u64 },
    /// Ask for the policy to be turned off. Effective after the delay.
    RequestDisable,
    /// Cancel a pending turn-off. Starts the cooldown.
    CancelDisable,
}

/// `Transaction::data` of a `TransactionType::SendAllowlist` transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedSendAllowlistAction {
    pub account: Address,
    pub action: SendAllowlistAction,
    /// Must equal the account's current `action_nonce`.
    pub action_nonce: u64,
    /// Lowercase hex ed25519 public key; must hash to `account`.
    pub public_key: String,
    /// Hex ed25519 signature over [`send_allowlist_action_message`].
    pub signature: String,
}

/// Every way an action or a send is refused. No panic anywhere in this module.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SendAllowlistError {
    #[error("send allowlist is not active at this height (ConsensusFeature::SendAllowlist)")]
    FeatureNotActive,
    #[error("malformed key or signature: {0}")]
    Malformed(&'static str),
    #[error("public key does not hash to the account")]
    KeyAddressMismatch,
    #[error("owner signature does not verify")]
    BadSignature,
    #[error("stale action nonce: expected {expected}, got {got}")]
    StaleNonce { expected: u64, got: u64 },
    #[error("action nonce exhausted")]
    NonceExhausted,
    #[error("the policy is already on")]
    AlreadyEnabled,
    #[error("the policy is off")]
    NotEnabled,
    #[error("delay {got} blocks is outside {min}..={max}")]
    DelayOutOfRange { got: u64, min: u64, max: u64 },
    #[error("delay can only be raised while the policy is on (current {current}, got {got})")]
    DelayNotRaised { current: u64, got: u64 },
    #[error("the list may not name the account itself (a self-send is always allowed)")]
    SelfListed,
    #[error("address {0} is listed twice")]
    Duplicate(Address),
    #[error("address {0} is already allowed or pending")]
    AlreadyListed(Address),
    #[error("address {0} is not on the list")]
    NotListed(Address),
    #[error("no pending addition for {0}")]
    NoPendingAddition(Address),
    #[error("no pending disable")]
    NoPendingDisable,
    #[error("a disable is already pending")]
    DisableAlreadyPending,
    #[error("list is full ({max} addresses including pending)")]
    ListFull { max: usize },
    #[error("too many pending additions ({max})")]
    TooManyPending { max: usize },
    #[error("cooldown after a cancellation: no new request before height {until}")]
    CooldownActive { until: u64 },
    #[error("height arithmetic overflow")]
    HeightOverflow,
}

/// Stable, non-serde encoding of an action for the signing preimage.
/// Persistence encoding is deliberately not reused: a change to the in-memory
/// layout must never silently change signed bytes.
fn encode_action_for_preimage(action: &SendAllowlistAction) -> Vec<u8> {
    fn push(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    let mut encoded = Vec::new();
    match action {
        SendAllowlistAction::Enable { add_delay_blocks, allowed } => {
            push(&mut encoded, &[0]);
            push(&mut encoded, &add_delay_blocks.to_le_bytes());
            push(&mut encoded, &(allowed.len() as u64).to_le_bytes());
            for address in allowed {
                push(&mut encoded, &address.0);
            }
        },
        SendAllowlistAction::RequestAdd { address } => {
            push(&mut encoded, &[1]);
            push(&mut encoded, &address.0);
        },
        SendAllowlistAction::CancelAdd { address } => {
            push(&mut encoded, &[2]);
            push(&mut encoded, &address.0);
        },
        SendAllowlistAction::Remove { address } => {
            push(&mut encoded, &[3]);
            push(&mut encoded, &address.0);
        },
        SendAllowlistAction::RaiseDelay { add_delay_blocks } => {
            push(&mut encoded, &[4]);
            push(&mut encoded, &add_delay_blocks.to_le_bytes());
        },
        SendAllowlistAction::RequestDisable => push(&mut encoded, &[5]),
        SendAllowlistAction::CancelDisable => push(&mut encoded, &[6]),
    }
    encoded
}

/// The exact bytes the account key signs. Every component is length-prefixed.
pub fn send_allowlist_action_message(
    chain_id: &str,
    account: &Address,
    action: &SendAllowlistAction,
    action_nonce: u64,
) -> Vec<u8> {
    fn push(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    let mut message = Vec::new();
    push(&mut message, SEND_ALLOWLIST_ACTION_DOMAIN_TAG);
    push(&mut message, chain_id.as_bytes());
    push(&mut message, &account.0);
    push(&mut message, &encode_action_for_preimage(action));
    push(&mut message, &action_nonce.to_le_bytes());
    message
}

impl SignedSendAllowlistAction {
    /// Sign `action` for `account` with `key` (CLI and tests).
    pub fn sign(
        chain_id: &str,
        key: &crate::crypto::signature::PrivateKey,
        action: SendAllowlistAction,
        action_nonce: u64,
    ) -> Self {
        let public_key = PublicKey::from_private_key(key);
        let account = Address::from_public_key(&public_key);
        let message = send_allowlist_action_message(chain_id, &account, &action, action_nonce);
        Self {
            account,
            action,
            action_nonce,
            public_key: public_key.to_hex(),
            signature: key.sign(&message).to_hex(),
        }
    }

    /// Stateless check: the key hashes to the account and the signature
    /// verifies over the preimage for `chain_id`. Mempool admission, the block
    /// classifier and [`apply`] all run this.
    pub fn verify(&self, chain_id: &str) -> Result<(), SendAllowlistError> {
        let public_key = PublicKey::from_hex(self.public_key.trim())
            .map_err(|_| SendAllowlistError::Malformed("public key is not hex ed25519"))?;
        if Address::from_public_key(&public_key) != self.account {
            return Err(SendAllowlistError::KeyAddressMismatch);
        }
        let signature = Signature::from_hex(self.signature.trim())
            .map_err(|_| SendAllowlistError::Malformed("signature is not hex ed25519"))?;
        let message =
            send_allowlist_action_message(chain_id, &self.account, &self.action, self.action_nonce);
        if !public_key.verify(&message, &signature) {
            return Err(SendAllowlistError::BadSignature);
        }
        Ok(())
    }

    /// Parse a `TransactionType::SendAllowlist` transaction. `None` for any
    /// other type, an oversized or unparsable payload, or a payload whose
    /// account is not the transaction's `sender_address`.
    pub fn parse(tx: &Transaction) -> Option<Self> {
        if tx.tx_type != TransactionType::SendAllowlist {
            return None;
        }
        let data = tx.data.as_ref()?;
        if data.len() > MAX_SEND_ALLOWLIST_PAYLOAD_BYTES {
            return None;
        }
        let parsed: Self = serde_json::from_str(data).ok()?;
        (parsed.account == tx.sender_address).then_some(parsed)
    }
}

fn validate_delay(add_delay_blocks: u64) -> Result<(), SendAllowlistError> {
    if !(MIN_ADD_DELAY_BLOCKS..=MAX_ADD_DELAY_BLOCKS).contains(&add_delay_blocks) {
        return Err(SendAllowlistError::DelayOutOfRange {
            got: add_delay_blocks,
            min: MIN_ADD_DELAY_BLOCKS,
            max: MAX_ADD_DELAY_BLOCKS,
        });
    }
    Ok(())
}

fn check_cooldown(policy: &SendAllowlistPolicy, height: u64) -> Result<(), SendAllowlistError> {
    match policy.requests_blocked_until_height {
        Some(until) if height < until => Err(SendAllowlistError::CooldownActive { until }),
        _ => Ok(()),
    }
}

fn start_cooldown(policy: &mut SendAllowlistPolicy, height: u64) -> Result<(), SendAllowlistError> {
    policy.requests_blocked_until_height = Some(
        height
            .checked_add(CANCEL_COOLDOWN_BLOCKS)
            .ok_or(SendAllowlistError::HeightOverflow)?,
    );
    Ok(())
}

/// Apply one signed action at `height`. The single entry point consensus
/// calls (`State::apply_protocol_transaction`'s `SendAllowlist` arm). All
/// validation precedes the single write of the account's entry, so a refused
/// action leaves `states` untouched.
pub fn apply(
    states: &mut BTreeMap<Address, AccountSendAllowlist>,
    signed: &SignedSendAllowlistAction,
    height: u64,
    chain_id: &str,
) -> Result<(), SendAllowlistError> {
    signed.verify(chain_id)?;
    let account = signed.account;
    let mut entry = states.get(&account).cloned().unwrap_or_default();
    if signed.action_nonce != entry.action_nonce {
        return Err(SendAllowlistError::StaleNonce {
            expected: entry.action_nonce,
            got: signed.action_nonce,
        });
    }
    let next_nonce = entry
        .action_nonce
        .checked_add(1)
        .ok_or(SendAllowlistError::NonceExhausted)?;
    entry.settle(height);

    match &signed.action {
        SendAllowlistAction::Enable { add_delay_blocks, allowed } => {
            if entry.policy.is_some() {
                return Err(SendAllowlistError::AlreadyEnabled);
            }
            validate_delay(*add_delay_blocks)?;
            if allowed.len() > MAX_ALLOWED_ADDRESSES {
                return Err(SendAllowlistError::ListFull { max: MAX_ALLOWED_ADDRESSES });
            }
            let mut set = BTreeSet::new();
            for address in allowed {
                if *address == account {
                    return Err(SendAllowlistError::SelfListed);
                }
                if !set.insert(*address) {
                    return Err(SendAllowlistError::Duplicate(*address));
                }
            }
            entry.policy = Some(SendAllowlistPolicy {
                enabled_at_height: height,
                add_delay_blocks: *add_delay_blocks,
                allowed: set,
                pending_additions: BTreeMap::new(),
                pending_disable: None,
                requests_blocked_until_height: None,
            });
        },
        SendAllowlistAction::RequestAdd { address } => {
            let policy = entry.policy.as_mut().ok_or(SendAllowlistError::NotEnabled)?;
            check_cooldown(policy, height)?;
            if *address == account {
                return Err(SendAllowlistError::SelfListed);
            }
            if policy.allowed.contains(address) || policy.pending_additions.contains_key(address) {
                return Err(SendAllowlistError::AlreadyListed(*address));
            }
            if policy.pending_additions.len() >= MAX_PENDING_ADDITIONS {
                return Err(SendAllowlistError::TooManyPending { max: MAX_PENDING_ADDITIONS });
            }
            if policy.allowed.len() + policy.pending_additions.len() >= MAX_ALLOWED_ADDRESSES {
                return Err(SendAllowlistError::ListFull { max: MAX_ALLOWED_ADDRESSES });
            }
            let effective_at_height = height
                .checked_add(policy.add_delay_blocks)
                .ok_or(SendAllowlistError::HeightOverflow)?;
            policy.pending_additions.insert(
                *address,
                PendingChange { requested_at_height: height, effective_at_height },
            );
        },
        SendAllowlistAction::CancelAdd { address } => {
            let policy = entry.policy.as_mut().ok_or(SendAllowlistError::NotEnabled)?;
            if policy.pending_additions.remove(address).is_none() {
                return Err(SendAllowlistError::NoPendingAddition(*address));
            }
            start_cooldown(policy, height)?;
        },
        SendAllowlistAction::Remove { address } => {
            let policy = entry.policy.as_mut().ok_or(SendAllowlistError::NotEnabled)?;
            let removed = policy.allowed.remove(address) | policy.pending_additions.remove(address).is_some();
            if !removed {
                return Err(SendAllowlistError::NotListed(*address));
            }
        },
        SendAllowlistAction::RaiseDelay { add_delay_blocks } => {
            let policy = entry.policy.as_mut().ok_or(SendAllowlistError::NotEnabled)?;
            validate_delay(*add_delay_blocks)?;
            if *add_delay_blocks <= policy.add_delay_blocks {
                return Err(SendAllowlistError::DelayNotRaised {
                    current: policy.add_delay_blocks,
                    got: *add_delay_blocks,
                });
            }
            policy.add_delay_blocks = *add_delay_blocks;
        },
        SendAllowlistAction::RequestDisable => {
            let policy = entry.policy.as_mut().ok_or(SendAllowlistError::NotEnabled)?;
            check_cooldown(policy, height)?;
            if policy.pending_disable.is_some() {
                return Err(SendAllowlistError::DisableAlreadyPending);
            }
            let effective_at_height = height
                .checked_add(policy.add_delay_blocks)
                .ok_or(SendAllowlistError::HeightOverflow)?;
            policy.pending_disable =
                Some(PendingChange { requested_at_height: height, effective_at_height });
        },
        SendAllowlistAction::CancelDisable => {
            let policy = entry.policy.as_mut().ok_or(SendAllowlistError::NotEnabled)?;
            if policy.pending_disable.take().is_none() {
                return Err(SendAllowlistError::NoPendingDisable);
            }
            start_cooldown(policy, height)?;
        },
    }

    entry.action_nonce = next_nonce;
    states.insert(account, entry);
    Ok(())
}

/// The policy restricting `account` at `height`, if any. Every value guard in
/// `State` goes through this or [`permits`].
pub fn restricting_policy<'a>(
    states: &'a BTreeMap<Address, AccountSendAllowlist>,
    account: &Address,
    height: u64,
) -> Option<&'a SendAllowlistPolicy> {
    states.get(account)?.policy_in_force(height)
}

/// Whether value may move from `from` to `to` at `height`. Always true for an
/// account with no policy in force.
pub fn permits(
    states: &BTreeMap<Address, AccountSendAllowlist>,
    from: &Address,
    to: &Address,
    height: u64,
) -> bool {
    restricting_policy(states, from, height).is_none_or(|policy| policy.permits(from, to, height))
}

/// Refusal text for a send the policy blocks. Stable prefix for clients.
pub fn destination_refusal(from: &Address, to: &Address, height: u64) -> String {
    format!(
        "SP_SEND_ALLOWLIST_DESTINATION_REFUSED: {from} has a send allowlist in force at height \
         {height} and {to} is not on it"
    )
}

/// Refusal text for a value movement whose destination cannot be listed.
pub fn movement_refusal(from: &Address, what: &str, height: u64) -> String {
    format!(
        "SP_SEND_ALLOWLIST_MOVEMENT_REFUSED: {from} has a send allowlist in force at height \
         {height}; {what} is refused while it is on"
    )
}

/// How a transaction type relates to value leaving the sending account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueMovement {
    /// Debits the account to a named destination. Refused unless the
    /// destination is permitted ([`permits`]).
    ToDestination,
    /// Debits the account to somewhere that cannot be put on a list (a burn,
    /// a pool, a deposit that is never refunded, a sponsorship deposit).
    /// Refused while a policy is in force.
    Unlistable,
    /// Moves value only inside the account (spendable <-> staked) or back
    /// into it. Allowed.
    WithinAccount,
    /// Credits only, or protocol-initiated (rewards, slashing), or no balance
    /// effect in the account model today. Allowed.
    NotADebit,
    /// Mixed: some actions route value elsewhere (checked per action at its
    /// own apply seam), the rest are within the account.
    PerAction,
}

/// The classification of every transaction type, deny-by-default for value
/// movement. Exhaustive on purpose: a new type does not compile until it is
/// classified here. Where the guard lives is named per arm.
pub const fn value_movement(tx_type: TransactionType) -> ValueMovement {
    use TransactionType as T;
    use ValueMovement as V;
    match tx_type {
        // `BalanceTransfer` is the one debit-to-destination transition; every
        // transparent transfer (ed25519, passkey or governance-authority
        // signed) reaches it. Guarded in its apply arm, so any future emitter
        // is covered too.
        T::Transfer => V::ToDestination,
        // Deposit into the shielded pool: the destination is hidden by design.
        // Guarded in `ApplyShieldedDeposit`. (Also disabled chain-wide, #1762.)
        T::ShieldedDeposit => V::Unlistable,
        // The proposal deposit is debited and held, never refunded. Guarded in
        // `CreateGovernanceProposal`. (Also disabled chain-wide, #2024.)
        T::GovernanceProposal => V::Unlistable,
        // Deposit/Update bind a payout account that receives the bond's
        // rewards: refused unless the payout account is permitted. Exit and
        // Withdraw return principal to the owner. Guarded in
        // `ApplyNodeBondLifecycle`.
        T::NodeBondLifecycle => V::PerAction,
        // Stake/unstake move spendable <-> locked inside the same account.
        T::Stake | T::Unstake => V::WithinAccount,
        // Spend shielded notes, not the transparent account balance; a note is
        // not attributable to an account, so there is nothing to bind.
        T::ConfidentialTransfer | T::ShieldedWithdrawal | T::RewardClaim => V::NotADebit,
        // Register/revoke a session key (#97) or a security key (#103): no value
        // moves. The spends they authorize are ordinary `Transfer`s and reach
        // the `BalanceTransfer` guard like any other signer.
        T::SessionKeyAction | T::SecurityKeyAction => V::NotADebit,
        // Credits, protocol records, or types with no state-machine effect on
        // account balances in this binary (the transition extractor and
        // `apply_protocol_transaction` emit nothing that debits for them).
        T::Regular
        | T::Coinbase
        | T::NftCreation
        | T::NftTransfer
        | T::NftExecution
        | T::DataAnalysis
        | T::DataCollection
        | T::DataStorage
        | T::EnvironmentalOffset
        | T::BridgeLock
        | T::BridgeUnlock
        | T::ContributionCommit
        | T::ContributionReveal
        | T::ForecastCommit
        | T::ForecastReveal
        | T::OracleAttestation
        | T::GovernanceVote
        | T::GovernanceExecute
        | T::GovernanceFinalize
        | T::GovernanceDelegate
        | T::TreasuryDeposit
        | T::TreasuryWithdraw
        | T::Reward
        | T::Layer2BatchSubmit
        | T::SlashingReport
        | T::OracleRegistration
        | T::StationRegistration
        | T::FeatureScheduleExtension
        | T::SlashEvidenceReport
        | T::NodeServiceBatch
        | T::NodeFraudReport
        | T::SendAllowlist => V::NotADebit,
    }
}

/// Read model for the node API and wallets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendAllowlistView {
    pub account: String,
    /// The nonce the next action must carry.
    pub next_action_nonce: u64,
    /// Whether sends are restricted at `at_height`.
    pub in_force: bool,
    pub at_height: u64,
    pub add_delay_blocks: Option<u64>,
    pub enabled_at_height: Option<u64>,
    /// In force at `at_height`, including pending additions that have matured.
    pub allowed: Vec<String>,
    /// Still waiting at `at_height`. Show these to the owner.
    pub pending_additions: Vec<PendingAdditionView>,
    pub pending_disable: Option<PendingChange>,
    pub requests_blocked_until_height: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAdditionView {
    pub address: String,
    pub requested_at_height: u64,
    pub effective_at_height: u64,
}

/// Build the read model for `account` as of `height` (normally tip + 1).
pub fn view(
    states: &BTreeMap<Address, AccountSendAllowlist>,
    account: &Address,
    height: u64,
) -> SendAllowlistView {
    let mut entry = states.get(account).cloned().unwrap_or_default();
    let next_action_nonce = entry.action_nonce;
    entry.settle(height);
    let policy = entry.policy.as_ref();
    SendAllowlistView {
        account: account.to_string(),
        next_action_nonce,
        in_force: policy.is_some(),
        at_height: height,
        add_delay_blocks: policy.map(|p| p.add_delay_blocks),
        enabled_at_height: policy.map(|p| p.enabled_at_height),
        allowed: policy
            .map(|p| p.allowed.iter().map(|a| a.to_string()).collect())
            .unwrap_or_default(),
        pending_additions: policy
            .map(|p| {
                p.pending_additions
                    .iter()
                    .map(|(address, pending)| PendingAdditionView {
                        address: address.to_string(),
                        requested_at_height: pending.requested_at_height,
                        effective_at_height: pending.effective_at_height,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        pending_disable: policy.and_then(|p| p.pending_disable),
        requests_blocked_until_height: policy.and_then(|p| p.requests_blocked_until_height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::PrivateKey;
    use rand::{SeedableRng, rngs::StdRng};

    const CHAIN: &str = "888";

    fn key(seed: u64) -> PrivateKey {
        PrivateKey::generate(&mut StdRng::seed_from_u64(seed))
    }
    fn addr(seed: u64) -> Address {
        Address::from_public_key(&PublicKey::from_private_key(&key(seed)))
    }
    fn other(byte: u8) -> Address {
        Address([byte; 20])
    }

    struct Owner {
        key: PrivateKey,
        account: Address,
        states: BTreeMap<Address, AccountSendAllowlist>,
    }
    impl Owner {
        fn new() -> Self {
            let key = key(1);
            let account = Address::from_public_key(&PublicKey::from_private_key(&key));
            Self { key, account, states: BTreeMap::new() }
        }
        fn nonce(&self) -> u64 {
            self.states.get(&self.account).map_or(0, |e| e.action_nonce)
        }
        fn act(&mut self, action: SendAllowlistAction, height: u64) -> Result<(), SendAllowlistError> {
            let signed = SignedSendAllowlistAction::sign(CHAIN, &self.key, action, self.nonce());
            apply(&mut self.states, &signed, height, CHAIN)
        }
        fn permits(&self, to: &Address, height: u64) -> bool {
            permits(&self.states, &self.account, to, height)
        }
    }

    fn enable(owner: &mut Owner, list: Vec<Address>, height: u64) {
        owner
            .act(SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: list }, height)
            .unwrap();
    }

    #[test]
    fn no_policy_permits_everything() {
        let owner = Owner::new();
        assert!(owner.permits(&other(9), 0));
        assert!(owner.permits(&other(9), u64::MAX));
    }

    #[test]
    fn enable_is_immediate_and_restricts_unlisted_destinations() {
        let mut owner = Owner::new();
        enable(&mut owner, vec![other(2)], 100);
        assert!(owner.permits(&other(2), 100), "listed address allowed in the same block");
        assert!(!owner.permits(&other(3), 100), "unlisted refused immediately");
        assert!(owner.permits(&owner.account.clone(), 100), "self-send always allowed");
        assert!(permits(&owner.states, &other(7), &other(3), 100), "other accounts unaffected");
    }

    #[test]
    fn addition_waits_exactly_the_delay() {
        let mut owner = Owner::new();
        enable(&mut owner, vec![], 100);
        owner.act(SendAllowlistAction::RequestAdd { address: other(4) }, 200).unwrap();
        let effective = 200 + MIN_ADD_DELAY_BLOCKS;
        assert!(!owner.permits(&other(4), 200));
        assert!(!owner.permits(&other(4), effective - 1), "one block early is refused");
        assert!(owner.permits(&other(4), effective), "inclusive at the effective height");
        let v = view(&owner.states, &owner.account, effective - 1);
        assert_eq!(v.pending_additions.len(), 1, "wallet sees the pending addition");
        assert_eq!(v.pending_additions[0].effective_at_height, effective);
        let v = view(&owner.states, &owner.account, effective);
        assert!(v.pending_additions.is_empty());
        assert_eq!(v.allowed, vec![other(4).to_string()]);
    }

    #[test]
    fn cancel_add_removes_pending_and_starts_cooldown() {
        let mut owner = Owner::new();
        enable(&mut owner, vec![], 100);
        owner.act(SendAllowlistAction::RequestAdd { address: other(4) }, 200).unwrap();
        owner.act(SendAllowlistAction::CancelAdd { address: other(4) }, 300).unwrap();
        assert!(!owner.permits(&other(4), 200 + MIN_ADD_DELAY_BLOCKS + 10));
        let until = 300 + CANCEL_COOLDOWN_BLOCKS;
        assert_eq!(
            owner.act(SendAllowlistAction::RequestAdd { address: other(4) }, until - 1),
            Err(SendAllowlistError::CooldownActive { until })
        );
        assert_eq!(
            owner.act(SendAllowlistAction::RequestDisable, until - 1),
            Err(SendAllowlistError::CooldownActive { until })
        );
        owner.act(SendAllowlistAction::RequestAdd { address: other(4) }, until).unwrap();
    }

    #[test]
    fn remove_is_immediate_for_allowed_and_pending() {
        let mut owner = Owner::new();
        enable(&mut owner, vec![other(2)], 100);
        owner.act(SendAllowlistAction::RequestAdd { address: other(4) }, 101).unwrap();
        owner.act(SendAllowlistAction::Remove { address: other(2) }, 102).unwrap();
        assert!(!owner.permits(&other(2), 102));
        owner.act(SendAllowlistAction::Remove { address: other(4) }, 103).unwrap();
        assert!(!owner.permits(&other(4), 101 + MIN_ADD_DELAY_BLOCKS));
        assert_eq!(
            owner.act(SendAllowlistAction::Remove { address: other(5) }, 104),
            Err(SendAllowlistError::NotListed(other(5)))
        );
    }

    #[test]
    fn disable_is_time_locked_and_cancellable() {
        let mut owner = Owner::new();
        enable(&mut owner, vec![], 100);
        owner.act(SendAllowlistAction::RequestDisable, 200).unwrap();
        let effective = 200 + MIN_ADD_DELAY_BLOCKS;
        assert!(!owner.permits(&other(9), effective - 1));
        assert!(owner.permits(&other(9), effective), "matured disable lifts the policy");
        // Cancel before maturity keeps the policy on for good.
        owner.act(SendAllowlistAction::CancelDisable, effective - 1).unwrap();
        assert!(!owner.permits(&other(9), effective + 10_000));
        assert_eq!(
            owner.act(SendAllowlistAction::CancelDisable, effective),
            Err(SendAllowlistError::NoPendingDisable)
        );
    }

    #[test]
    fn matured_disable_clears_policy_and_allows_re_enable_keeping_nonce() {
        let mut owner = Owner::new();
        enable(&mut owner, vec![other(2)], 100);
        owner.act(SendAllowlistAction::RequestDisable, 200).unwrap();
        let effective = 200 + MIN_ADD_DELAY_BLOCKS;
        assert_eq!(
            owner.act(SendAllowlistAction::RequestAdd { address: other(3) }, effective),
            Err(SendAllowlistError::NotEnabled)
        );
        let nonce_before = owner.nonce();
        enable(&mut owner, vec![], effective);
        assert_eq!(owner.nonce(), nonce_before + 1, "nonce never resets");
        assert!(!owner.permits(&other(2), effective), "old list does not survive a disable");
    }

    #[test]
    fn delay_bounds_and_only_upward_changes() {
        let mut owner = Owner::new();
        assert!(matches!(
            owner.act(SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS - 1, allowed: vec![] }, 1),
            Err(SendAllowlistError::DelayOutOfRange { .. })
        ));
        assert!(matches!(
            owner.act(SendAllowlistAction::Enable { add_delay_blocks: MAX_ADD_DELAY_BLOCKS + 1, allowed: vec![] }, 1),
            Err(SendAllowlistError::DelayOutOfRange { .. })
        ));
        enable(&mut owner, vec![], 1);
        assert_eq!(
            owner.act(SendAllowlistAction::RaiseDelay { add_delay_blocks: MIN_ADD_DELAY_BLOCKS }, 2),
            Err(SendAllowlistError::DelayNotRaised { current: MIN_ADD_DELAY_BLOCKS, got: MIN_ADD_DELAY_BLOCKS })
        );
        owner.act(SendAllowlistAction::RaiseDelay { add_delay_blocks: DEFAULT_ADD_DELAY_BLOCKS }, 3).unwrap();
        owner.act(SendAllowlistAction::RequestAdd { address: other(4) }, 10).unwrap();
        assert!(!owner.permits(&other(4), 10 + MIN_ADD_DELAY_BLOCKS));
        assert!(owner.permits(&other(4), 10 + DEFAULT_ADD_DELAY_BLOCKS));
        assert_eq!(
            owner.act(SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![] }, 11),
            Err(SendAllowlistError::AlreadyEnabled)
        );
    }

    #[test]
    fn list_shape_rules() {
        let mut owner = Owner::new();
        let me = owner.account;
        assert_eq!(
            owner.act(SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![me] }, 1),
            Err(SendAllowlistError::SelfListed)
        );
        assert_eq!(
            owner.act(SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![other(2), other(2)] }, 1),
            Err(SendAllowlistError::Duplicate(other(2)))
        );
        let too_many: Vec<Address> = (0..=MAX_ALLOWED_ADDRESSES as u8).map(other).collect();
        assert!(matches!(
            owner.act(SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: too_many }, 1),
            Err(SendAllowlistError::ListFull { .. })
        ));
        enable(&mut owner, vec![other(2)], 1);
        assert_eq!(
            owner.act(SendAllowlistAction::RequestAdd { address: other(2) }, 2),
            Err(SendAllowlistError::AlreadyListed(other(2)))
        );
        for i in 0..MAX_PENDING_ADDITIONS as u8 {
            owner.act(SendAllowlistAction::RequestAdd { address: other(100 + i) }, 3).unwrap();
        }
        assert_eq!(
            owner.act(SendAllowlistAction::RequestAdd { address: other(200) }, 3),
            Err(SendAllowlistError::TooManyPending { max: MAX_PENDING_ADDITIONS })
        );
    }

    #[test]
    fn refused_action_changes_nothing_and_does_not_consume_the_nonce() {
        let mut owner = Owner::new();
        enable(&mut owner, vec![], 1);
        let before = owner.states.clone();
        assert!(owner.act(SendAllowlistAction::CancelDisable, 2).is_err());
        assert_eq!(owner.states, before);
    }

    #[test]
    fn replay_and_domain_separation() {
        let mut owner = Owner::new();
        let signed = SignedSendAllowlistAction::sign(
            CHAIN,
            &owner.key,
            SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![] },
            0,
        );
        apply(&mut owner.states, &signed, 1, CHAIN).unwrap();
        // Same bytes again: the nonce has moved on.
        assert_eq!(
            apply(&mut owner.states, &signed, 2, CHAIN),
            Err(SendAllowlistError::StaleNonce { expected: 1, got: 0 })
        );
        // Another chain id: signature does not verify.
        let mut fresh = BTreeMap::new();
        assert_eq!(apply(&mut fresh, &signed, 1, "777"), Err(SendAllowlistError::BadSignature));
        // Swap the action under the same signature.
        let mut swapped = signed.clone();
        swapped.action = SendAllowlistAction::Enable { add_delay_blocks: MIN_ADD_DELAY_BLOCKS, allowed: vec![other(6)] };
        assert_eq!(apply(&mut BTreeMap::new(), &swapped, 1, CHAIN), Err(SendAllowlistError::BadSignature));
        // Claim another account with the same key.
        let mut stolen = signed.clone();
        stolen.account = addr(2);
        assert_eq!(apply(&mut BTreeMap::new(), &stolen, 1, CHAIN), Err(SendAllowlistError::KeyAddressMismatch));
        // A third party's key cannot act on the owner's account.
        let thief = SignedSendAllowlistAction::sign(CHAIN, &key(3), SendAllowlistAction::RequestDisable, 1);
        let mut forged = thief.clone();
        forged.account = owner.account;
        assert_eq!(apply(&mut owner.states, &forged, 3, CHAIN), Err(SendAllowlistError::KeyAddressMismatch));
    }

    #[test]
    fn every_action_has_a_distinct_preimage() {
        let account = other(1);
        let actions = [
            SendAllowlistAction::Enable { add_delay_blocks: 5, allowed: vec![] },
            SendAllowlistAction::Enable { add_delay_blocks: 5, allowed: vec![other(2)] },
            SendAllowlistAction::RequestAdd { address: other(2) },
            SendAllowlistAction::CancelAdd { address: other(2) },
            SendAllowlistAction::Remove { address: other(2) },
            SendAllowlistAction::RaiseDelay { add_delay_blocks: 5 },
            SendAllowlistAction::RequestDisable,
            SendAllowlistAction::CancelDisable,
        ];
        let mut seen = BTreeSet::new();
        for action in &actions {
            for nonce in [0u64, 1] {
                assert!(seen.insert(send_allowlist_action_message(CHAIN, &account, action, nonce)));
            }
        }
        // Pinned prefix: the domain tag is the first length-prefixed field.
        let message = send_allowlist_action_message(CHAIN, &account, &actions[6], 0);
        assert_eq!(&message[8..8 + SEND_ALLOWLIST_ACTION_DOMAIN_TAG.len()], SEND_ALLOWLIST_ACTION_DOMAIN_TAG);
    }

    /// Property: for random action sequences, a destination is only ever
    /// permitted if it was in the initial list, or its addition request is at
    /// least `add_delay_blocks` old, or a disable request is that old.
    #[test]
    fn property_no_destination_is_permitted_before_its_delay() {
        use rand::Rng;
        for case in 0..64u64 {
            let mut rng = StdRng::seed_from_u64(case);
            let mut owner = Owner::new();
            let initial: Vec<Address> = vec![other(1)];
            enable(&mut owner, initial.clone(), 0);
            let mut height = 0u64;
            let mut requested: BTreeMap<Address, u64> = BTreeMap::new();
            let mut disable_requested: Option<u64> = None;
            for _ in 0..40 {
                height += rng.gen_range(0..400);
                let target = other(rng.gen_range(1..6));
                let action = match rng.gen_range(0..6) {
                    0 => SendAllowlistAction::RequestAdd { address: target },
                    1 => SendAllowlistAction::CancelAdd { address: target },
                    2 => SendAllowlistAction::Remove { address: target },
                    3 => SendAllowlistAction::RequestDisable,
                    4 => SendAllowlistAction::CancelDisable,
                    _ => SendAllowlistAction::RaiseDelay { add_delay_blocks: MIN_ADD_DELAY_BLOCKS + rng.gen_range(0..50) },
                };
                let ok = owner.act(action.clone(), height).is_ok();
                if ok {
                    match action {
                        SendAllowlistAction::RequestAdd { address } => { requested.insert(address, height); },
                        SendAllowlistAction::RequestDisable => disable_requested = Some(height),
                        SendAllowlistAction::CancelDisable => disable_requested = None,
                        _ => {},
                    }
                }
                for probe in 1..6u8 {
                    let to = other(probe);
                    if owner.permits(&to, height) {
                        let by_list = initial.contains(&to);
                        let by_age = requested.get(&to).is_some_and(|h| height >= h + MIN_ADD_DELAY_BLOCKS);
                        let by_disable = disable_requested.is_some_and(|h| height >= h + MIN_ADD_DELAY_BLOCKS);
                        assert!(by_list || by_age || by_disable, "case {case}: {to} permitted at {height} too early");
                    }
                }
            }
        }
    }

    #[test]
    fn every_transaction_type_is_classified() {
        // Exhaustiveness is compile-time; this pins the value-moving set so a
        // reclassification is a reviewed, visible change.
        use TransactionType as T;
        assert_eq!(value_movement(T::Transfer), ValueMovement::ToDestination);
        assert_eq!(value_movement(T::ShieldedDeposit), ValueMovement::Unlistable);
        assert_eq!(value_movement(T::GovernanceProposal), ValueMovement::Unlistable);
        assert_eq!(value_movement(T::NodeBondLifecycle), ValueMovement::PerAction);
        assert_eq!(value_movement(T::Stake), ValueMovement::WithinAccount);
        assert_eq!(value_movement(T::Unstake), ValueMovement::WithinAccount);
        for t in [T::ConfidentialTransfer, T::ShieldedWithdrawal, T::RewardClaim, T::Reward, T::Coinbase, T::SendAllowlist] {
            assert_eq!(value_movement(t), ValueMovement::NotADebit, "{t}");
        }
    }

    #[test]
    fn parse_binds_sender_and_type() {
        let owner = Owner::new();
        let signed = SignedSendAllowlistAction::sign(CHAIN, &owner.key, SendAllowlistAction::RequestDisable, 0);
        let mut tx = Transaction::new(vec![], vec![]);
        tx.tx_type = TransactionType::SendAllowlist;
        tx.sender_address = owner.account;
        tx.data = Some(serde_json::to_string(&signed).unwrap());
        assert_eq!(SignedSendAllowlistAction::parse(&tx), Some(signed.clone()));
        tx.sender_address = other(9);
        assert_eq!(SignedSendAllowlistAction::parse(&tx), None, "sender must be the account");
        tx.sender_address = owner.account;
        tx.tx_type = TransactionType::Transfer;
        assert_eq!(SignedSendAllowlistAction::parse(&tx), None);
        tx.tx_type = TransactionType::SendAllowlist;
        tx.data = Some("x".repeat(MAX_SEND_ALLOWLIST_PAYLOAD_BYTES + 1));
        assert_eq!(SignedSendAllowlistAction::parse(&tx), None);
    }
}
