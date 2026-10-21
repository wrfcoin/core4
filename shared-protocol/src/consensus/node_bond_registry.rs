//! Consensus epoch geometry and bond liability for Node-pay Slice 2.
//! Heights use checked arithmetic throughout. The withdrawal buffer delays
//! principal release without extending the fraud inclusion deadline.

use super::node_share_contracts::{NODE_BOND_WITHDRAWAL_BUFFER_BLOCKS, NODE_EPOCH_BLOCKS, node_epoch_blocks};

/// Epochs are aligned to the feature's immutable activation height.
pub fn epoch_at(activation_height: u64, height: u64) -> Option<u64> {
    height
        .checked_sub(activation_height)
        .map(|offset| offset / node_epoch_blocks())
}

pub fn epoch_start(activation_height: u64, epoch: u64) -> Option<u64> {
    activation_height.checked_add(epoch.checked_mul(node_epoch_blocks())?)
}

pub fn epoch_close(activation_height: u64, epoch: u64) -> Option<u64> {
    epoch_start(activation_height, epoch)?.checked_add(node_epoch_blocks() - 1)
}

/// Owner-ratified 2026-10-11: C_last is the close of the epoch containing
/// the exit effective height. A short interval since exit cannot truncate
/// liability for that complete service epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BondExitBoundary {
    pub last_service_epoch_close: u64,
    pub challenge_deadline: u64,
    pub earliest_withdrawal_height: u64,
}

impl BondExitBoundary {
    pub fn checked(
        activation_height: u64,
        exit_effective_height: u64,
        challenge_window: u64,
    ) -> Option<Self> {
        let last_epoch = epoch_at(activation_height, exit_effective_height)?;
        let last_service_epoch_close = epoch_close(activation_height, last_epoch)?;
        let challenge_deadline = last_service_epoch_close.checked_add(challenge_window)?;
        let earliest_withdrawal_height = challenge_deadline
            .checked_add(NODE_BOND_WITHDRAWAL_BUFFER_BLOCKS)?
            .checked_add(1)?
            .max(exit_effective_height.checked_add(1)?);
        Some(Self {
            last_service_epoch_close,
            challenge_deadline,
            earliest_withdrawal_height,
        })
    }

    pub fn slashable_at(self, carrying_height: u64) -> bool {
        carrying_height <= self.challenge_deadline
    }

    pub fn withdrawable_at(self, carrying_height: u64) -> bool {
        carrying_height >= self.earliest_withdrawal_height
    }
}

use super::node_share_contracts::{
    MAX_NODE_REGISTRATIONS, MAX_NODE_REGISTRY_BYTES, NODE_BOND_WRF, NodeBondAction,
    NodeBondAuthorization, SignedNodeBondAuthorization, TESTNET_CHALLENGE_BLOCKS,
};
use crate::blockchain::state::{Account, State, StateError};
use crate::common::types::{Address, Balance};
use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use crate::crypto::domain_impls::BodyBuilder;
use crate::crypto::domain_signing::{DomainSeparated, verify_domain_for};
use crate::crypto::signature::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const NODE_BOND_UNITS: u64 = NODE_BOND_WRF * crate::common::constants::WRF_UNITS_PER_COIN;
pub const MAX_NODE_BOND_LIFECYCLE_TXS_PER_BLOCK: usize = 16;
pub const MAX_RETAINED_NODE_EPOCHS: usize =
    ((TESTNET_CHALLENGE_BLOCKS + NODE_BOND_WITHDRAWAL_BUFFER_BLOCKS + NODE_EPOCH_BLOCKS - 1)
        / NODE_EPOCH_BLOCKS
        + 1) as usize;

/// Frozen history covers the chain's challenge and withdrawal buffer, plus
/// its current epoch. The legacy constant above remains the testnet wire pin.
pub fn max_retained_node_epochs(genesis_chain_id: Option<&str>) -> usize {
    ((super::node_share_contracts::node_challenge_blocks(genesis_chain_id)
        + NODE_BOND_WITHDRAWAL_BUFFER_BLOCKS + node_epoch_blocks() - 1)
        / node_epoch_blocks() + 1) as usize
}

/// Transport container only. Every approval signs the existing explicit
/// NodeBondAuthorization subject, never this serde/bincode container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeBondLifecycle {
    pub signed: SignedNodeBondAuthorization,
    pub owner_key: [u8; 32],
    pub governance_key: Option<[u8; 32]>,
    pub governance_signature: Vec<u8>,
    pub payout_key: Option<[u8; 32]>,
    pub payout_signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeBondRegistration {
    pub binding: NodeBondAuthorization,
    pub pending_binding: Option<NodeBondAuthorization>,
    pub registered_epoch: u64,
    pub last_change_epoch: u64,
    pub exit_effective_height: Option<u64>,
    pub principal_units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenNodeEpoch {
    pub roster_sequence: u64,
    pub parent_hash: [u8; 32],
    pub bindings: BTreeMap<[u8; 32], NodeBondAuthorization>,
}

/// Some(empty registry) is the durable activation/layout marker. None is
/// byte-absent from pre-activation root and legacy persistence encodings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeBondRegistry {
    pub activation_height: u64,
    pub genesis_hash: [u8; 32],
    pub block_height: u64,
    pub lifecycle_tx_count: u64,
    pub sequence: u64,
    pub registrations: BTreeMap<[u8; 32], NodeBondRegistration>,
    pub frozen_epochs: BTreeMap<u64, FrozenNodeEpoch>,
}

/// Protocol escrow has no private key or owner-selected address. V2 is
/// pre-activation only; the unscheduled feature leaves all OFF pins unchanged.
/// The domain
/// separates it from ordinary public-key-derived accounts.
pub fn node_bond_escrow_address(deployment_id: &[u8; 32]) -> Address {
    let body = BodyBuilder::new()
        .bytes_field(b"wrfcoin.node_bond_escrow.v2")
        .bytes_field(deployment_id).finish();
    let digest = crate::crypto::hash::hash_bytes(&body);
    Address::new(digest.as_bytes()[..20].try_into().expect("fixed hash prefix"))
}

/// Include future/historical producer bindings, not only today's committee.
/// Existing accounts are separately refused by the fresh-escrow rule.
/// Every committed source of non-transaction account credits, including
/// deferred obligations and historical/future bindings. Arbitrary transaction
/// recipients are fenced by the transition guard before any side effect.
pub fn is_protocol_credited_address(state: &State, address: &Address) -> bool {
    let identity = |id: &str| {
        id.parse::<Address>().ok().as_ref() == Some(address)
            || PublicKey::from_hex(id).ok().map(|k| Address::from_public_key(&k)).as_ref() == Some(address)
    };
    *address == super::node_fraud::forfeiture_address()
        || state.effective_faucet_address().as_ref() == Some(address)
        || crate::crypto::chain_binding::LEGACY_FAUCET_BINDINGS.iter().filter_map(|r| r.address).any(identity)
        || state.producer_set_history.iter().flat_map(|c| &c.members).any(|m| identity(&m.payout_address) || public(&m.public_key).ok().is_some_and(|k| &Address::from_public_key(&k) == address))
        || state.oracle_set.iter().any(|id| identity(id))
        || state.attester_settlement_due.values().flat_map(|d| &d.eligible).any(|id| identity(id))
        || state.station_registry.entries().any(|(_, station)| identity(&station.owner_address))
        || state.station_owner_ledger.values().filter_map(|l| l.pending_recovery.as_ref()).any(|r| identity(&r.to_owner_address))
        || state.contribution_reward_records.values().any(|r| &r.address == address)
        || state.node_bond_registry.as_ref().is_some_and(|r| {
            r.registrations.values().any(|r| &r.binding.payout_account == address || r.pending_binding.as_ref().is_some_and(|b| &b.payout_account == address))
                || r.frozen_epochs.values().flat_map(|e| e.bindings.values()).any(|b| &b.payout_account == address)
        })
        || state.node_settlement.as_ref().is_some_and(|s| s.epochs.values().flat_map(|e| e.eligible.values()).any(|a| a == address))
}

fn refused() -> StateError {
    StateError::Governance("SP_NODE_BOND_REFUSED".into())
}
fn public(key: &[u8; 32]) -> Result<PublicKey, StateError> {
    PublicKey::from_bytes(key).map_err(|_| refused())
}
fn approval(
    a: &NodeBondAuthorization,
    key: &[u8; 32],
    sig: &[u8],
    identity: &ChainIdentity,
) -> Result<(), StateError> {
    let sig = Signature::from_bytes(sig).map_err(|_| refused())?;
    if !verify_domain_for(&public(key)?, a, &sig, identity, SigningProtocolVersion::V2) {
        return Err(refused());
    }
    Ok(())
}

#[cfg(feature = "test-support")]
std::thread_local! {
    static LIFECYCLE_VERIFY_ATTEMPTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Thread-local observer at the lifecycle signature boundary; absent in production.
#[cfg(feature = "test-support")]
pub fn lifecycle_verify_attempts() -> u64 {
    LIFECYCLE_VERIFY_ATTEMPTS.with(|count| count.get())
}

impl NodeBondLifecycle {
    pub fn parse(tx: &crate::blockchain::transaction::Transaction) -> Option<Self> {
        let bytes = tx.data.as_ref()?;
        if bytes.len() > MAX_NODE_REGISTRY_BYTES {
            return None;
        }
        let parsed: Self = serde_json::from_str(bytes).ok()?;
        if tx.sender_address != parsed.signed.authorization.owner_account
            || tx.nonce != parsed.signed.authorization.expected_nonce
        {
            return None;
        }
        Some(parsed)
    }
    pub fn verify(
        &self,
        identity: &ChainIdentity,
        genesis: &[u8; 32],
        governance_keys: &[String],
    ) -> Result<(), StateError> {
        let a = &self.signed.authorization;
        if Address::from_public_key(&public(&self.owner_key)?) != a.owner_account {
            return Err(refused());
        }
        #[cfg(feature = "test-support")]
        LIFECYCLE_VERIFY_ATTEMPTS.with(|count| count.set(count.get() + 1));
        self.signed
            .verify_contract(identity, genesis, &self.owner_key)
            .map_err(|_| refused())?;
        if matches!(a.action, NodeBondAction::Deposit | NodeBondAction::Update) {
            let governance_key = self.governance_key.as_ref().ok_or_else(refused)?;
            if !governance_keys
                .iter()
                .any(|key| key == &hex::encode(governance_key))
            {
                return Err(refused());
            }
            approval(a, governance_key, &self.governance_signature, identity)?;
            let payout_key = self.payout_key.as_ref().ok_or_else(refused)?;
            if Address::from_public_key(&public(payout_key)?) != a.payout_account
                || *payout_key == a.server_key
                || *payout_key == a.consumer_key
            {
                return Err(refused());
            }
            approval(a, payout_key, &self.payout_signature, identity)?;
        } else if self.governance_key.is_some()
            || !self.governance_signature.is_empty()
            || self.payout_key.is_some()
            || !self.payout_signature.is_empty()
        {
            return Err(refused());
        }
        Ok(())
    }
}

impl NodeBondRegistry {
    pub fn begin(
        activation_height: u64,
        genesis_hash: [u8; 32],
        height: u64,
        parent_hash: [u8; 32],
    ) -> Result<Self, StateError> {
        if height != activation_height || epoch_close(activation_height, 0).is_none() {
            return Err(refused());
        }
        let mut registry = Self {
            activation_height,
            genesis_hash,
            block_height: height,
            lifecycle_tx_count: 0,
            sequence: 0,
            registrations: BTreeMap::new(),
            frozen_epochs: BTreeMap::new(),
        };
        registry.freeze_epoch(0, parent_hash);
        Ok(registry)
    }
    pub fn advance(&mut self, height: u64, parent_hash: [u8; 32]) -> Result<(), StateError> {
        self.advance_with_challenge(height, parent_hash, TESTNET_CHALLENGE_BLOCKS)
    }
    pub fn advance_with_challenge(&mut self, height: u64, parent_hash: [u8; 32], challenge: u64) -> Result<(), StateError> {
        let epoch = epoch_at(self.activation_height, height).ok_or_else(refused)?;
        let start = epoch_start(self.activation_height, epoch).ok_or_else(refused)?;
        if height == self.block_height {
            if height == start
                && self.frozen_epochs.get(&epoch).map(|e| e.parent_hash) != Some(parent_hash)
            {
                return Err(refused());
            }
            return Ok(());
        }
        if height != self.block_height.checked_add(1).ok_or_else(refused)? {
            return Err(refused());
        }
        if height == start {
            self.freeze_epoch(epoch, parent_hash);
        }
        if !self.frozen_epochs.contains_key(&epoch) {
            return Err(refused());
        }
        self.block_height = height;
        self.lifecycle_tx_count = 0;
        // Retain the epoch through its full challenge+withdrawal buffer.
        self.frozen_epochs.retain(|e, _| {
            epoch_close(self.activation_height, *e)
                .and_then(|c| c.checked_add(challenge))
                .and_then(|c| c.checked_add(NODE_BOND_WITHDRAWAL_BUFFER_BLOCKS))
                .is_none_or(|end| height <= end)
        });
        Ok(())
    }
    fn freeze_epoch(&mut self, epoch: u64, parent_hash: [u8; 32]) {
        let mut bindings = BTreeMap::new();
        for (id, record) in &mut self.registrations {
            if let Some(pending) = &record.pending_binding {
                if pending.effective_epoch <= epoch {
                    record.binding = pending.clone();
                    record.pending_binding = None;
                }
            }
            if record.principal_units == NODE_BOND_UNITS && record.binding.effective_epoch <= epoch
                && record.exit_effective_height.is_none_or(|exit| {
                    epoch_start(self.activation_height, epoch).is_some_and(|start| start < exit)
                })
            {
                bindings.insert(*id, record.binding.clone());
            }
        }
        self.frozen_epochs.insert(
            epoch,
            FrozenNodeEpoch {
                roster_sequence: self.sequence,
                parent_hash,
                bindings,
            },
        );
    }
    pub fn is_escrow(&self, address: &Address) -> bool {
        self.registrations
            .values()
            .any(|r| r.binding.escrow_account == *address)
    }
    pub fn canonical_body(&self) -> Vec<u8> {
        let mut b = BodyBuilder::new()
            .u64_field(self.activation_height)
            .bytes_field(&self.genesis_hash)
            .u64_field(self.block_height)
            .u64_field(self.lifecycle_tx_count)
            .u64_field(self.sequence)
            .u64_field(self.registrations.len() as u64);
        for (id, r) in &self.registrations {
            b = b
                .bytes_field(id)
                .bytes_field(&r.binding.canonical_body())
                .u64_field(r.registered_epoch)
                .u64_field(r.last_change_epoch)
                .u64_field(r.principal_units)
                .u32_field(u32::from(r.exit_effective_height.is_some()))
                .u64_field(r.exit_effective_height.unwrap_or(0))
                .u32_field(u32::from(r.pending_binding.is_some()));
            if let Some(pending) = &r.pending_binding {
                b = b.bytes_field(&pending.canonical_body());
            }
        }
        b = b.u64_field(self.frozen_epochs.len() as u64);
        for (epoch, frozen) in &self.frozen_epochs {
            b = b
                .u64_field(*epoch)
                .u64_field(frozen.roster_sequence)
                .bytes_field(&frozen.parent_hash)
                .u64_field(frozen.bindings.len() as u64);
            for (id, binding) in &frozen.bindings {
                b = b.bytes_field(id).bytes_field(&binding.canonical_body());
            }
        }
        b.finish()
    }
}

/// Validate the WAL input against the archived block, including the real
/// parent commitment. A transition's claimed hash is never its own authority.
pub fn validate_block_inputs(
    block: &crate::blockchain::block::Block,
    transitions: &[crate::blockchain::state::StateTransition],
    active: bool,
) -> Result<(), StateError> {
    use crate::blockchain::{state::StateTransition, transaction::TransactionType};
    let starts: Vec<_> = transitions
        .iter()
        .filter_map(|t| match t {
            StateTransition::BeginNodeServiceBlock {
                block_height,
                parent_hash,
            } => Some((*block_height, *parent_hash)),
            _ => None,
        })
        .collect();
    let carried: Vec<_> = block
        .transactions
        .iter()
        .filter(|tx| tx.tx_type == TransactionType::NodeBondLifecycle)
        .collect();
    if carried.len() > MAX_NODE_BOND_LIFECYCLE_TXS_PER_BLOCK {
        return Err(refused());
    }
    if active {
        if starts != vec![(block.height, *block.prev_block_hash.as_bytes())]
            || !matches!(
                transitions.first(),
                Some(StateTransition::BeginNodeServiceBlock { .. })
            )
        {
            return Err(refused());
        }
    } else if !starts.is_empty() || !carried.is_empty() {
        return Err(refused());
    }
    let applied: Vec<_> = transitions
        .iter()
        .filter_map(|t| match t {
            StateTransition::ApplyNodeBondLifecycle {
                payload,
                block_height,
            } => Some((payload, *block_height)),
            _ => None,
        })
        .collect();
    if carried.len() != applied.len() {
        return Err(refused());
    }
    for (tx, (payload, height)) in carried.iter().zip(applied) {
        if height != block.height || NodeBondLifecycle::parse(tx).as_ref() != Some(payload) {
            return Err(refused());
        }
    }
    crate::consensus::node_receipt_registry::validate_block_inputs(block, transitions, active)?;
    crate::consensus::node_fraud::validate_block_inputs(block, transitions, active)?;
    Ok(())
}

fn role_wallet_aliases(
    a: &NodeBondAuthorization,
    b: &NodeBondAuthorization,
) -> Result<bool, StateError> {
    let a_online = [
        Address::from_public_key(&public(&a.server_key)?),
        Address::from_public_key(&public(&a.consumer_key)?),
    ];
    let b_online = [
        Address::from_public_key(&public(&b.server_key)?),
        Address::from_public_key(&public(&b.consumer_key)?),
    ];
    let a_wallets = [a.owner_account, a.payout_account, a.escrow_account];
    let b_wallets = [b.owner_account, b.payout_account, b.escrow_account];
    Ok(a_online.iter().any(|key| b_wallets.contains(key))
        || b_online.iter().any(|key| a_wallets.contains(key)))
}

/// Clone-and-commit makes signature, nonce, amount and arithmetic refusals
/// atomic even for direct state-transition callers. Account balances are the
/// existing supply representation; escrow is never a mint or burn.
pub(crate) fn apply_lifecycle(
    state: &mut State,
    payload: &NodeBondLifecycle,
    height: u64,
    identity: &ChainIdentity,
    genesis: &[u8; 32],
    governance_keys: &[String],
) -> Result<(), StateError> {
    payload.verify(identity, genesis, governance_keys)?;
    let a = &payload.signed.authorization;
    use crate::accounts::authorization::{
        AuthorizationRule, AuthorizationView, DEBIT_TIME_UNPLUMBED_2168, authorization_for,
    };
    let owner_rule = authorization_for(
        &state.declared(&a.owner_account),
        height,
        DEBIT_TIME_UNPLUMBED_2168,
    );
    if !matches!(owner_rule, AuthorizationRule::SignerIsSender) {
        return Err(refused());
    }
    if matches!(a.action, NodeBondAction::Deposit | NodeBondAction::Update)
        && state.node_bond_registry.as_ref().is_some_and(|r| r.is_escrow(&a.payout_account)) {
        return Err(refused());
    }
    let mut registry = state.node_bond_registry.clone().ok_or_else(refused)?;
    if registry.block_height != height
        || registry.genesis_hash != *genesis
        || registry.lifecycle_tx_count >= MAX_NODE_BOND_LIFECYCLE_TXS_PER_BLOCK as u64
    {
        return Err(refused());
    }
    let epoch = epoch_at(registry.activation_height, height).ok_or_else(refused)?;
    let mut owner = state
        .accounts
        .get(&a.owner_account)
        .cloned()
        .ok_or(StateError::AccountNotFound)?;
    if a.owner_account == super::node_fraud::forfeiture_address() || a.payout_account == super::node_fraud::forfeiture_address()
        || state.node_fraud.as_ref().is_some_and(|f| f.jailed.contains(&a.deployment_id)) { return Err(refused()); }
    if owner.nonce != a.expected_nonce || registry.is_escrow(&a.owner_account) {
        return Err(StateError::InvalidNonce);
    }
    owner.nonce = owner
        .nonce
        .checked_add(1)
        .ok_or(StateError::NonceOverflow)?;
    let mut escrow_change = None;
    match a.action {
        NodeBondAction::Deposit => {
            if a.amount_units != NODE_BOND_UNITS
                || a.effective_epoch != epoch.checked_add(1).ok_or_else(refused)?
                || registry.registrations.len() >= MAX_NODE_REGISTRATIONS
                || registry.registrations.contains_key(&a.deployment_id)
                || a.escrow_account != node_bond_escrow_address(&a.deployment_id)
                || is_protocol_credited_address(state, &a.escrow_account)
                || state.accounts.contains_key(&a.escrow_account)
            {
                return Err(refused());
            }
            let amount = Balance::new(u128::from(NODE_BOND_UNITS));
            owner.spendable_balance = owner
                .spendable_balance
                .checked_sub(amount)
                .ok_or(StateError::InsufficientSpendableBalance)?;
            owner.balance = owner
                .balance
                .checked_sub(amount)
                .ok_or(StateError::InsufficientBalance)?;
            let mut escrow = Account::new(amount);
            escrow.spendable_balance = Balance::new(0);
            escrow.stake_locked = amount;
            escrow.stake_required = amount;
            escrow_change = Some(escrow);
            registry.registrations.insert(
                a.deployment_id,
                NodeBondRegistration {
                    binding: a.clone(),
                    pending_binding: None,
                    registered_epoch: epoch,
                    last_change_epoch: epoch,
                    exit_effective_height: None,
                    principal_units: NODE_BOND_UNITS,
                },
            );
        }
        NodeBondAction::Update | NodeBondAction::Exit | NodeBondAction::Withdraw => {
            let r = registry
                .registrations
                .get_mut(&a.deployment_id)
                .ok_or_else(refused)?;
            if a.owner_account != r.binding.owner_account
                || a.escrow_account != r.binding.escrow_account
            {
                return Err(refused());
            }
            if matches!(a.action, NodeBondAction::Update | NodeBondAction::Exit) {
                if a.amount_units != 0
                    || a.effective_epoch != epoch.checked_add(1).ok_or_else(refused)?
                    || r.last_change_epoch == epoch
                    || r.exit_effective_height.is_some()
                {
                    return Err(refused());
                }
                r.last_change_epoch = epoch;
                if a.action == NodeBondAction::Update {
                    r.pending_binding = Some(a.clone());
                } else {
                    if a.server_key != r.binding.server_key
                        || a.consumer_key != r.binding.consumer_key
                        || a.payout_account != r.binding.payout_account
                        || a.service_alias != r.binding.service_alias
                    {
                        return Err(refused());
                    }
                    r.exit_effective_height = Some(
                        epoch_start(registry.activation_height, a.effective_epoch)
                            .ok_or_else(refused)?,
                    );
                }
            } else {
                let exit = r.exit_effective_height.ok_or_else(refused)?;
                let boundary = BondExitBoundary::checked(
                    registry.activation_height,
                    exit,
                    super::node_share_contracts::node_challenge_blocks(state.genesis_emission_chain_id()),
                )
                .ok_or_else(refused)?;
                if !boundary.withdrawable_at(height)
                    || a.amount_units != r.principal_units
                    || a.effective_epoch != epoch
                    || a.server_key != r.binding.server_key
                    || a.consumer_key != r.binding.consumer_key
                    || a.payout_account != r.binding.payout_account
                    || a.service_alias != r.binding.service_alias
                {
                    return Err(refused());
                }
                let mut escrow = state
                    .accounts
                    .get(&a.escrow_account)
                    .cloned()
                    .ok_or_else(refused)?;
                let amount = Balance::new(u128::from(r.principal_units));
                if escrow.balance != amount
                    || escrow.stake_locked != amount
                    || escrow.spendable_balance != Balance::new(0)
                {
                    return Err(refused());
                }
                owner.balance = owner
                    .balance
                    .checked_add(amount)
                    .ok_or(StateError::ArithmeticOverflow)?;
                owner.spendable_balance = owner
                    .spendable_balance
                    .checked_add(amount)
                    .ok_or(StateError::ArithmeticOverflow)?;
                escrow.balance = Balance::new(0);
                escrow.stake_locked = Balance::new(0);
                escrow.stake_required = Balance::new(0);
                escrow_change = Some(escrow);
                registry.registrations.remove(&a.deployment_id);
            }
        }
    }
    // Include current, pending and retained historical bindings in alias checks.
    let others: Vec<_> = registry
        .registrations
        .iter()
        .filter(|(id, _)| **id != a.deployment_id)
        .flat_map(|(_, r)| std::iter::once(&r.binding).chain(r.pending_binding.iter()))
        .chain(
            registry
                .frozen_epochs
                .values()
                .flat_map(|e| e.bindings.iter())
                .filter(|(id, _)| **id != a.deployment_id)
                .map(|(_, b)| b),
        )
        .collect();
    for b in &others {
        if role_wallet_aliases(a, b)? {
            return Err(refused());
        }
    }
    if others.iter().any(|b| {
        b.owner_account == a.owner_account
            || b.escrow_account == a.escrow_account
            || b.payout_account == a.payout_account
            || b.service_alias == a.service_alias
            || [b.server_key, b.consumer_key].contains(&a.server_key)
            || [b.server_key, b.consumer_key].contains(&a.consumer_key)
    }) {
        return Err(refused());
    }
    if matches!(a.action, NodeBondAction::Deposit | NodeBondAction::Update) {
        let role_keys = state.signing_keys.all_registered_public_keys();
        let mut forbidden: Vec<[u8; 32]> = role_keys
            .iter()
            .chain(state.oracle_set.iter())
            .chain(governance_keys.iter())
            .filter_map(|k| hex::decode(k).ok()?.try_into().ok())
            .collect();
        for change in &state.producer_set_history {
            forbidden.extend(change.members.iter().map(|m| m.public_key));
            if change
                .members
                .iter()
                .any(|m| m.payout_address.parse::<Address>().ok() == Some(a.payout_account))
            {
                return Err(refused());
            }
        }
        if forbidden.iter().any(|k| {
            *k == a.server_key
                || *k == a.consumer_key
                || public(k).is_ok_and(|k| Address::from_public_key(&k) == a.payout_account)
        }) {
            return Err(refused());
        }
    }
    registry.sequence = registry
        .sequence
        .checked_add(1)
        .ok_or(StateError::ArithmeticOverflow)?;
    registry.lifecycle_tx_count += 1;
    state.accounts.insert(a.owner_account, owner);
    if let Some(escrow) = escrow_change {
        state.accounts.insert(a.escrow_account, escrow);
    }
    state.node_bond_registry = Some(registry);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::node_share_contracts::TESTNET_CHALLENGE_BLOCKS;

    #[test]
    fn owner_ratified_withdrawal_buffer_boundaries() {
        // Independent numerical oracle: A=2,000, exit becomes effective at
        // epoch 2's start, C_last=5,071, deadline=6,095, withdrawal=6,196.
        let boundary = BondExitBoundary::checked(2_000, 4_048, TESTNET_CHALLENGE_BLOCKS)
            .expect("representable boundary");
        assert_eq!(boundary.last_service_epoch_close, 5_071);
        assert_eq!(boundary.challenge_deadline, 6_095);
        assert_eq!(boundary.earliest_withdrawal_height, 6_196);
        assert!(boundary.slashable_at(6_095), "inclusive fraud deadline");
        assert!(!boundary.slashable_at(6_096));
        assert!(!boundary.withdrawable_at(6_095), "deadline is locked");
        assert!(
            !boundary.withdrawable_at(6_195),
            "deadline+buffer is locked"
        );
        assert!(boundary.withdrawable_at(6_196), "deadline+buffer+1 unlocks");
        assert!(
            !boundary.withdrawable_at(5_173),
            "exit-based deadline is too early"
        );
    }

    #[test]
    fn epoch_geometry_and_exit_boundaries_fail_closed_on_overflow() {
        assert_eq!(epoch_at(2_000, 1_999), None);
        assert_eq!(epoch_at(2_000, 2_000), Some(0));
        assert_eq!(epoch_at(2_000, 3_023), Some(0));
        assert_eq!(epoch_at(2_000, 3_024), Some(1));
        assert_eq!(epoch_start(u64::MAX, 1), None);
        assert_eq!(epoch_close(u64::MAX, 0), None);
        assert_eq!(BondExitBoundary::checked(2_000, 1_999, 1_024), None);
        assert_eq!(BondExitBoundary::checked(0, u64::MAX, 1_024), None);
        assert_eq!(BondExitBoundary::checked(0, 0, u64::MAX), None);
    }
}
