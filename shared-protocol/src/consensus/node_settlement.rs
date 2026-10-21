//! Derive-only unminted Node authorization. Never a transaction or metadata authority.
use super::{
    node_bond_registry::{NODE_BOND_UNITS, epoch_at, epoch_close, max_retained_node_epochs},
    node_share_contracts::node_challenge_blocks,
    proof_of_publishing::{EmissionRole, MAX_SUPPLY},
};
use crate::{
    blockchain::state::{State, StateError},
    common::types::{Address, Balance},
    crypto::domain_impls::BodyBuilder,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EpochStatus {
    Open,
    Provisional,
    Paid,
    Unissued,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeEpochBudget {
    pub scheduled: u128,
    pub reserved: u128,
    pub unissued: u128,
    pub maturity_height: u64,
    pub eligible: BTreeMap<[u8; 32], Address>,
    pub payouts: BTreeMap<[u8; 32], u128>,
    pub provisional: BTreeMap<[u8; 32], u128>,
    pub status: EpochStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NodeSettlement {
    pub last_finalized_height: Option<u64>,
    pub outstanding: u128,
    pub epochs: BTreeMap<u64, NodeEpochBudget>,
}

fn invalid() -> StateError {
    StateError::Governance("SP_NODE_RESERVE_INVARIANT".into())
}

/// Inclusive challenge deadline, followed by the first redemption block.
pub fn maturity_height(activation: u64, epoch: u64, chain: &str) -> Option<u64> {
    let challenge = node_challenge_blocks(Some(chain));
    epoch_close(activation, epoch)?
        .checked_add(challenge)?
        .checked_add(1)
}

pub fn rotated_split(
    epoch: u64,
    budget: u128,
    ids: impl Iterator<Item = [u8; 32]>,
) -> BTreeMap<[u8; 32], u128> {
    let ids: Vec<_> = ids.collect();
    let n = ids.len();
    if n == 0 {
        return BTreeMap::new();
    }
    let q = budget / n as u128;
    let r = (budget % n as u128) as usize;
    let start = (epoch % n as u64) as usize;
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| (id, q + u128::from((i + n - start) % n < r)))
        .collect()
}

impl NodeSettlement {
    pub fn canonical_body(&self) -> Vec<u8> {
        let mut b = BodyBuilder::new()
            .u64_field(self.last_finalized_height.is_some() as u64)
            .u64_field(self.last_finalized_height.unwrap_or(0))
            .bytes_field(&self.outstanding.to_le_bytes())
            .u64_field(self.epochs.len() as u64);
        for (e, r) in &self.epochs {
            b = b
                .u64_field(*e)
                .bytes_field(&r.scheduled.to_le_bytes())
                .bytes_field(&r.reserved.to_le_bytes())
                .bytes_field(&r.unissued.to_le_bytes())
                .u64_field(r.maturity_height)
                .u64_field(match r.status {
                    EpochStatus::Open => 0,
                    EpochStatus::Provisional => 1,
                    EpochStatus::Paid => 2,
                    EpochStatus::Unissued => 3,
                })
                .u64_field(r.eligible.len() as u64);
            for (id, address) in &r.eligible {
                b = b.bytes_field(id).bytes_field(&address.0);
            }
            b = b.u64_field(r.provisional.len() as u64);
            for (id, amount) in &r.provisional {
                b = b.bytes_field(id).bytes_field(&amount.to_le_bytes());
            }
            b = b.u64_field(r.payouts.len() as u64);
            for (id, amount) in &r.payouts {
                b = b.bytes_field(id).bytes_field(&amount.to_le_bytes());
            }
        }
        b.finish()
    }
    pub fn validate(&self, state: &State, height: u64) -> Result<(), StateError> {
        let registry = state.node_bond_registry.as_ref().ok_or_else(invalid)?;
        let chain = state.genesis_emission_chain_id().ok_or_else(invalid)?;
        if self.epochs.len() > max_retained_node_epochs(Some(chain)) + 1
            || self.last_finalized_height.is_some_and(|h| h > height)
        {
            return Err(invalid());
        }
        let mut outstanding = 0u128;
        for (epoch, r) in &self.epochs {
            if r.reserved > r.scheduled
                || r.eligible.len() > super::node_share_contracts::MAX_NODE_REGISTRATIONS
                || Some(r.maturity_height)
                    != maturity_height(registry.activation_height, *epoch, chain)
            {
                return Err(invalid());
            }
            if matches!(r.status, EpochStatus::Provisional | EpochStatus::Paid)
                && (r.eligible.is_empty()
                    || r.provisional
                        != rotated_split(*epoch, r.reserved, r.eligible.keys().copied()))
            {
                return Err(invalid());
            }
            if r.status == EpochStatus::Open
                && (!r.eligible.is_empty() || !r.provisional.is_empty() || !r.payouts.is_empty())
            {
                return Err(invalid());
            }
            let paid = r
                .payouts
                .values()
                .try_fold(0u128, |a, v| a.checked_add(*v))
                .ok_or_else(invalid)?;
            match r.status {
                EpochStatus::Open | EpochStatus::Provisional => {
                    outstanding = outstanding.checked_add(r.reserved).ok_or_else(invalid)?;
                    if paid != 0 || r.unissued != r.scheduled - r.reserved {
                        return Err(invalid());
                    }
                },
                EpochStatus::Paid => {
                    if paid.checked_add(r.unissued) != Some(r.scheduled)
                        || paid > r.reserved
                        || r.payouts.iter().any(|(id, amount)| r.provisional.get(id) != Some(amount))
                    {
                        return Err(invalid());
                    }
                },
                EpochStatus::Unissued => {
                    if paid != 0 || r.unissued != r.scheduled {
                        return Err(invalid());
                    }
                },
            }
        }
        if outstanding != self.outstanding
            || state
                .checked_total_minted_supply()?
                .checked_add(outstanding)
                .is_none_or(|s| s > MAX_SUPPLY)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

/// Common finalization: redeem old reserves, reserve after existing roles, close.
/// All fallible calculations precede commit; the bounded store is copied once
/// here, never per recipient, and is untouched when finalizing the same height.
pub fn finalize(state: &mut State, height: u64) -> Result<(), StateError> {
    let Some(store) = state.node_settlement.as_ref() else {
        return Ok(());
    };
    if store.last_finalized_height == Some(height) {
        let actual = state.checked_total_minted_supply()?;
        if state
            .emission_ledger
            .cached_supply()
            .is_some_and(|cached| cached != actual)
        {
            return Err(invalid());
        }
        state.emission_ledger.set_cached_supply(actual);
        state.emission_ledger.restore_node_reservation();
        return state
            .node_settlement
            .as_ref()
            .ok_or_else(invalid)?
            .validate(state, height);
    }
    let bonds = state.node_bond_registry.as_ref().ok_or_else(invalid)?;
    let activation = bonds.activation_height;
    if bonds.block_height != height
        || store
            .last_finalized_height
            .map_or(height != activation, |h| h.checked_add(1) != Some(height))
    {
        return Err(invalid());
    }
    store.validate(state, height)?;
    let actual = state.checked_total_minted_supply()?;
    if state
        .emission_ledger
        .cached_supply()
        .is_some_and(|cached| cached != actual)
    {
        return Err(invalid());
    }
    state.emission_ledger.set_cached_supply(actual);
    let mut next = store.clone();
    let mut credits: BTreeMap<Address, u128> = BTreeMap::new();
    let mut redeemed = 0u128;
    for (epoch, r) in &mut next.epochs {
        if r.status != EpochStatus::Provisional || height < r.maturity_height {
            continue;
        }
        // Frozen allocations survive key changes. A sanctioned pending share
        // returns to unissued authorization; it is never redistributed or minted.
        if r.eligible.is_empty() {
            r.status = EpochStatus::Unissued;
            r.unissued = r.scheduled;
        } else {
            r.payouts = r.provisional.iter().filter(|(id, _)| !state.node_fraud.as_ref().is_some_and(|f| f.jailed.contains(*id)))
                .map(|(id, amount)| (*id, *amount)).collect();
            let paid = r.payouts.values().try_fold(0u128, |sum, amount| sum.checked_add(*amount)).ok_or_else(invalid)?;
            r.unissued = r.scheduled.checked_sub(paid).ok_or_else(invalid)?;
            for (id, amount) in &r.payouts {
                let address = *r.eligible.get(id).ok_or_else(invalid)?;
                let total = credits.entry(address).or_default();
                *total = total.checked_add(*amount).ok_or_else(invalid)?;
            }
            redeemed = redeemed.checked_add(paid).ok_or_else(invalid)?;
            r.status = if r.payouts.is_empty() { EpochStatus::Unissued } else { EpochStatus::Paid };
        }
        next.outstanding = next
            .outstanding
            .checked_sub(r.reserved)
            .ok_or_else(invalid)?;
    }
    let after_redemption = actual.checked_add(redeemed).ok_or_else(invalid)?;
    if after_redemption
        .checked_add(next.outstanding)
        .is_none_or(|v| v > MAX_SUPPLY)
    {
        return Err(invalid());
    }
    let scheduled = state.emission_ledger.partition().slice(EmissionRole::Node);
    let available = MAX_SUPPLY
        .saturating_sub(after_redemption)
        .saturating_sub(next.outstanding);
    let reserve = scheduled.min(available);
    if !state
        .emission_ledger
        .can_charge(EmissionRole::Node, reserve)
    {
        return Err(invalid());
    }
    let epoch = epoch_at(activation, height).ok_or_else(invalid)?;
    let r = next.epochs.entry(epoch).or_insert(NodeEpochBudget {
        scheduled: 0,
        reserved: 0,
        unissued: 0,
        maturity_height: maturity_height(
            activation,
            epoch,
            state.genesis_emission_chain_id().ok_or_else(invalid)?,
        )
        .ok_or_else(invalid)?,
        eligible: BTreeMap::new(),
        payouts: BTreeMap::new(),
        provisional: BTreeMap::new(),
        status: EpochStatus::Open,
    });
    if r.status != EpochStatus::Open {
        return Err(invalid());
    }
    r.scheduled = r.scheduled.checked_add(scheduled).ok_or_else(invalid)?;
    r.reserved = r.reserved.checked_add(reserve).ok_or_else(invalid)?;
    r.unissued = r.scheduled - r.reserved;
    next.outstanding = next.outstanding.checked_add(reserve).ok_or_else(invalid)?;
    if Some(height) == epoch_close(activation, epoch) {
        let frozen = bonds.frozen_epochs.get(&epoch).ok_or_else(invalid)?;
        let receipts = state.node_receipt_registry.as_ref().ok_or_else(invalid)?;
        for (id, binding) in &frozen.bindings {
            if !state.node_fraud.as_ref().is_some_and(|f| f.jailed.contains(id)) && receipts.has_service(epoch, id)
                && bonds
                    .registrations
                    .get(id)
                    .is_some_and(|record| record.principal_units == NODE_BOND_UNITS)
            {
                r.eligible.insert(*id, binding.payout_account);
            }
        }
        if r.eligible.is_empty() {
            next.outstanding = next
                .outstanding
                .checked_sub(r.reserved)
                .ok_or_else(invalid)?;
            r.unissued = r.scheduled;
            r.status = EpochStatus::Unissued;
        } else {
            r.provisional = rotated_split(epoch, r.reserved, r.eligible.keys().copied());
            r.status = EpochStatus::Provisional;
        }
    }
    next.last_finalized_height = Some(height);
    next.epochs.retain(|e, r| {
        bonds.frozen_epochs.contains_key(e)
            || matches!(r.status, EpochStatus::Open | EpochStatus::Provisional)
    });
    let mut accounts = Vec::with_capacity(credits.len());
    for (address, amount) in credits {
        let mut account = state
            .accounts
            .get(&address)
            .cloned()
            .unwrap_or_else(|| crate::blockchain::state::Account::new(Balance::new(0)));
        let amount = Balance::new(amount);
        account.earned_balance_total = account
            .earned_balance_total
            .checked_add(amount)
            .ok_or_else(invalid)?;
        account.balance = account.balance.checked_add(amount).ok_or_else(invalid)?;
        let lock = account
            .stake_required
            .saturating_sub(account.stake_locked)
            .min(amount);
        account.stake_locked = account.stake_locked.checked_add(lock).ok_or_else(invalid)?;
        account.spendable_balance = account
            .spendable_balance
            .checked_add(amount.saturating_sub(lock))
            .ok_or_else(invalid)?;
        accounts.push((address, account));
    }
    let minted = state
        .supply_authorized_minting
        .checked_add(redeemed)
        .ok_or_else(invalid)?;
    if !state.emission_ledger.reserve_node(reserve) {
        return Err(invalid());
    }
    for (address, account) in accounts {
        state.accounts.insert(address, account);
    }
    state.supply_authorized_minting = minted;
    state.emission_ledger.set_cached_supply(after_redemption);
    state.node_settlement = Some(next);
    state
        .node_settlement
        .as_ref()
        .ok_or_else(invalid)?
        .validate(state, height)
}
