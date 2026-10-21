//! Attributable Node-service fraud. No unauthenticated name is slash authority.
use super::{
    node_bond_registry::{epoch_at, epoch_close},
    node_receipt_registry::{NodeReceiptSubmission, batch_hash, frozen_roster_hash},
    node_share_contracts::*,
};
use crate::{
    blockchain::{
        state::{Account, State, StateError, StateTransition},
        transaction::{Transaction, TransactionType},
    },
    common::types::{Address, Balance},
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion, protocol_version_at},
        domain_impls::BodyBuilder,
        domain_signing::verify_domain_for,
        hash::hash_bytes,
        signature::{PublicKey, Signature},
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_NODE_FRAUD_REPORTS_PER_BLOCK: u64 = 8;

/// Uses the existing funded batch authorization subject with an explicit fraud
/// purpose commitment. An admission signature cannot be repurposed as a report.
pub type NodeFraudReport = NodeReceiptSubmission;

pub fn evidence_hash(p: &NodeFraudReport) -> Result<[u8; 32], StateError> {
    Ok(*hash_bytes(
        &BodyBuilder::new()
            .bytes_field(b"wrfcoin.node_fraud_report.v1")
            .bytes_field(&batch_hash(&p.batch).map_err(|_| refused())?)
            .finish(),
    )
    .as_bytes())
}
fn refused() -> StateError {
    StateError::Governance("SP_NODE_FRAUD_REFUSED".into())
}

pub fn parse(tx: &Transaction) -> Option<NodeFraudReport> {
    if tx.tx_type != TransactionType::NodeFraudReport {
        return None;
    }
    let data = tx.data.as_deref()?;
    if data.len() > MAX_NODE_FRAUD_BYTES * 2 {
        return None;
    }
    let bytes = hex::decode(data).ok()?;
    let count = u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?);
    if !(1..=2).contains(&count) {
        return None;
    }
    let p = NodeFraudReport::from_wire(&bytes).ok()?;
    (p.authorization.sender == tx.sender_address && p.authorization.nonce == tx.nonce).then_some(p)
}

pub fn verify_authorization(
    p: &NodeFraudReport,
    identity: &ChainIdentity,
    genesis: &[u8; 32],
) -> Result<(), StateError> {
    if !(1..=2).contains(&p.batch.claims.len())
        || p.to_wire().map_err(|_| refused())?.len() > MAX_NODE_FRAUD_BYTES
        || p.authorization.genesis_hash != *genesis
        || p.authorization.batch_hash != evidence_hash(p)?
    {
        return Err(refused());
    }
    let key = PublicKey::from_bytes(&p.owner_key).map_err(|_| refused())?;
    let sig = Signature::from_bytes(&p.owner_signature).map_err(|_| refused())?;
    if p.authorization.sender != Address::from_public_key(&key)
        || !verify_domain_for(
            &key,
            &p.authorization,
            &sig,
            identity,
            SigningProtocolVersion::V2,
        )
    {
        return Err(refused());
    }
    Ok(())
}

pub fn forfeiture_address() -> Address {
    let digest = hash_bytes(b"wrfcoin.node_nonspendable_forfeiture.v1");
    Address::new(
        digest.as_bytes()[..20]
            .try_into()
            .expect("fixed hash prefix"),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NodeFraudState {
    pub block_height: u64,
    pub report_count: u64,
    /// Permanent jail until a separately reviewed owner-authorized recovery.
    /// Registrations remain in the bounded registry; no churn erases liability.
    pub jailed: BTreeSet<[u8; 32]>,
    /// One terminal per responsible registration/epoch, bounded by frozen history.
    pub terminals: BTreeMap<(u64, [u8; 32]), [u8; 32]>,
    pub confiscated: u128,
    /// Pre-activation balance adopted once, never counted as confiscation.
    pub adopted: u128,
}
impl NodeFraudState {
    pub fn advance(&mut self, bonds: &super::node_bond_registry::NodeBondRegistry) {
        if self.block_height != bonds.block_height {
            self.report_count = 0;
        }
        self.block_height = bonds.block_height;
        self.terminals
            .retain(|(epoch, _), _| bonds.frozen_epochs.contains_key(epoch));
    }
    pub fn canonical_body(&self) -> Vec<u8> {
        let mut b = BodyBuilder::new()
            .u64_field(self.block_height)
            .u64_field(self.report_count)
            .bytes_field(&self.confiscated.to_le_bytes())
            .bytes_field(&self.adopted.to_le_bytes())
            .u64_field(self.jailed.len() as u64);
        for id in &self.jailed {
            b = b.bytes_field(id);
        }
        b = b.u64_field(self.terminals.len() as u64);
        for ((epoch, id), hash) in &self.terminals {
            b = b.u64_field(*epoch).bytes_field(id).bytes_field(hash);
        }
        b.finish()
    }
    pub fn validate(&self, state: &State, height: u64) -> Result<(), StateError> {
        let bonds = state.node_bond_registry.as_ref().ok_or_else(refused)?;
        if self.confiscated
            != (self.jailed.len() as u128) * u128::from(super::node_bond_registry::NODE_BOND_UNITS)
            || self.block_height != height
            || self.report_count > MAX_NODE_FRAUD_REPORTS_PER_BLOCK
            || self.jailed.len() > MAX_NODE_REGISTRATIONS
            || self.terminals.len() > bonds.frozen_epochs.len() * MAX_NODE_REGISTRATIONS
            || self.jailed.iter().any(|id| {
                !bonds
                    .registrations
                    .get(id)
                    .is_some_and(|r| r.principal_units == 0)
            })
            || self.terminals.keys().any(|(epoch, id)| {
                !bonds.frozen_epochs.contains_key(epoch) || !self.jailed.contains(id)
            })
        {
            return Err(refused());
        }
        let total = self
            .confiscated
            .checked_add(self.adopted)
            .ok_or(StateError::ArithmeticOverflow)?;
        match state.accounts.get(&forfeiture_address()) {
            None if total == 0 => {},
            Some(account)
                if account.balance == Balance::new(total)
                    && account.stake_locked == account.balance
                    && account.stake_required == account.balance
                    && account.spendable_balance.0 == 0 => {},
            _ => return Err(refused()),
        }
        Ok(())
    }
}

fn valid_ticket_signature(
    c: &BondedServiceClaim,
    key: &[u8; 32],
    identity: &ChainIdentity,
) -> bool {
    let Ok(key) = PublicKey::from_bytes(key) else {
        return false;
    };
    let Ok(sig) = Signature::from_bytes(&c.server_signature) else {
        return false;
    };
    verify_domain_for(&key, &c.ticket, &sig, identity, SigningProtocolVersion::V2)
}

/// Verify exact committed material and frozen context before considering blame.
/// Historical escrow availability is checked at debit, not as signature authority.
fn binding<'a>(
    state: &'a State,
    c: &BondedServiceClaim,
    height: u64,
    identity: &ChainIdentity,
) -> Result<(&'a NodeBondAuthorization, &'a NodeBondAuthorization), StateError> {
    c.validate_wire_bounds().map_err(|_| refused())?;
    let bonds = state.node_bond_registry.as_ref().ok_or_else(refused)?;
    let t = &c.ticket;
    let frozen = bonds
        .frozen_epochs
        .get(&t.context.epoch)
        .ok_or_else(refused)?;
    let server = frozen.bindings.get(&t.server_id).ok_or_else(refused)?;
    let consumer = frozen.bindings.get(&t.consumer_id).ok_or_else(refused)?;
    let deadline = epoch_close(bonds.activation_height, t.context.epoch)
        .and_then(|h| h.checked_add(node_challenge_blocks(state.genesis_emission_chain_id())))
        .ok_or_else(refused)?;
    if height > deadline
        || height < c.receipt.receipt.chain_height
        || (t.context.genesis_hash != bonds.genesis_hash
            || t.context.chain_id != identity.chain_id
            || t.context.network_id != identity.network_id)
        || t.context.roster_sequence != frozen.roster_sequence
        || t.context.roster_hash != frozen_roster_hash(frozen)
        || t.context.epoch_start_parent != frozen.parent_hash
        || t.server_id == t.consumer_id
        || server.owner_account == consumer.owner_account
        || c.receipt.receipt.consumer_pubkey != consumer.consumer_key
        || c.receipt.receipt.server_id != server.service_alias
        || epoch_at(bonds.activation_height, c.receipt.receipt.chain_height)
            != Some(t.context.epoch)
        || t.signed_receipt_hash != signed_receipt_hash(&c.receipt)
        || t.signed_manifest_hash != signed_manifest_hash(&c.manifest)
    {
        return Err(refused());
    }
    Ok((server, consumer))
}

pub fn responsible(
    state: &State,
    p: &NodeFraudReport,
    height: u64,
    identity: &ChainIdentity,
) -> Result<BTreeSet<[u8; 32]>, StateError> {
    let first = p.batch.claims.first().ok_or_else(refused)?;
    let (server, consumer) = binding(state, first, height, identity)?;
    let mut ids = BTreeSet::new();
    match p.batch.claims.as_slice() {
        [c] => {
            // Authenticate the server's exact payment assertion BEFORE inspecting
            // the invalid consumer signature. The named consumer is never blamed.
            if !valid_ticket_signature(c, &server.server_key, identity)
                || c.receipt
                    .verify_signature(
                        identity,
                        protocol_version_at(c.receipt.receipt.chain_height),
                    )
                    .is_ok()
            {
                return Err(refused());
            }
            ids.insert(server.deployment_id);
        },
        // Signing protection and this fraud verifier must remain in lockstep.
        [a, b] => {
            binding(state, b, height, identity)?;
            if a.ticket.context != b.ticket.context
                || a.ticket.server_id != b.ticket.server_id
                || a.ticket.consumer_id != b.ticket.consumer_id
                || a.ticket.ticket_id != b.ticket.ticket_id
                || (a.ticket.signed_receipt_hash == b.ticket.signed_receipt_hash
                    && a.ticket.signed_manifest_hash == b.ticket.signed_manifest_hash)
            {
                return Err(refused());
            }
            if valid_ticket_signature(a, &server.server_key, identity)
                && valid_ticket_signature(b, &server.server_key, identity)
            {
                ids.insert(server.deployment_id);
            }
            if a.verify_consumer_endorsement(identity, &consumer.consumer_key)
                .is_ok()
                && b.verify_consumer_endorsement(identity, &consumer.consumer_key)
                    .is_ok()
            {
                ids.insert(consumer.deployment_id);
            }
            if ids.is_empty() {
                return Err(refused());
            }
        },
        _ => return Err(refused()),
    }
    Ok(ids)
}

/// Prepare every account/registry mutation before committing an atomic transfer.
pub fn apply(state: &mut State, p: &NodeFraudReport, height: u64) -> Result<(), StateError> {
    use crate::accounts::authorization::{
        AuthorizationRule, AuthorizationView, DEBIT_TIME_UNPLUMBED_2168, authorization_for,
    };
    if !state
        .node_share_activation_height()
        .is_some_and(|a| height >= a)
    {
        return Err(refused());
    }
    let c = crate::governance::schedule_extension::schedule_extension_authority()
        .ok_or_else(refused)?
        .context();
    let identity = ChainIdentity::new(c.chain_id, c.network_id);
    let bonds = state.node_bond_registry.as_ref().ok_or_else(refused)?;
    verify_authorization(p, &identity, &bonds.genesis_hash)?;
    if bonds.block_height != height
        || !matches!(
            authorization_for(
                &state.declared(&p.authorization.sender),
                height,
                DEBIT_TIME_UNPLUMBED_2168
            ),
            AuthorizationRule::SignerIsSender
        )
        || bonds.is_escrow(&p.authorization.sender)
        || p.authorization.sender == forfeiture_address()
    {
        return Err(refused());
    }
    let mut reporter = state
        .accounts
        .get(&p.authorization.sender)
        .cloned()
        .ok_or_else(refused)?;
    if reporter.nonce != p.authorization.nonce || reporter.spendable_balance.0 == 0 {
        return Err(refused());
    }
    reporter.nonce = reporter
        .nonce
        .checked_add(1)
        .ok_or(StateError::NonceOverflow)?;
    let ids = responsible(state, p, height, &identity)?;
    let mut fraud = state.node_fraud.clone().ok_or_else(refused)?;
    fraud.validate(state, height)?;
    if fraud.block_height != height || fraud.report_count >= MAX_NODE_FRAUD_REPORTS_PER_BLOCK {
        return Err(refused());
    }
    let mut bonds = bonds.clone();
    let mut sink = state
        .accounts
        .get(&forfeiture_address())
        .cloned()
        .unwrap_or_else(|| Account::new(Balance::new(0)));
    let mut escrows = Vec::new();
    let epoch = p.batch.claims[0].ticket.context.epoch;
    for id in ids {
        if fraud.jailed.contains(&id) {
            continue;
        } // deterministic sanction no-op
        let historical = bonds.frozen_epochs[&epoch]
            .bindings
            .get(&id)
            .ok_or_else(refused)?;
        let record = bonds.registrations.get_mut(&id).ok_or_else(refused)?;
        if record.binding.escrow_account != historical.escrow_account {
            return Err(refused());
        }
        let mut escrow = state
            .accounts
            .get(&historical.escrow_account)
            .cloned()
            .ok_or_else(refused)?;
        let amount = Balance::new(u128::from(record.principal_units));
        if amount.0 == 0
            || escrow.balance != amount
            || escrow.stake_locked != amount
            || escrow.stake_required != amount
            || escrow.spendable_balance.0 != 0
        {
            return Err(refused());
        }
        escrow.balance = Balance::new(0);
        escrow.stake_locked = Balance::new(0);
        escrow.stake_required = Balance::new(0);
        sink.balance = sink
            .balance
            .checked_add(amount)
            .ok_or(StateError::ArithmeticOverflow)?;
        sink.stake_locked = sink
            .stake_locked
            .checked_add(amount)
            .ok_or(StateError::ArithmeticOverflow)?;
        sink.stake_required = sink
            .stake_required
            .checked_add(amount)
            .ok_or(StateError::ArithmeticOverflow)?;
        sink.spendable_balance = Balance::new(0);
        fraud.confiscated = fraud
            .confiscated
            .checked_add(amount.0)
            .ok_or(StateError::ArithmeticOverflow)?;
        record.principal_units = 0;
        record.pending_binding = None;
        fraud.jailed.insert(id);
        fraud.terminals.insert((epoch, id), evidence_hash(p)?);
        escrows.push((historical.escrow_account, escrow));
    }
    fraud.report_count += 1;
    state.accounts.insert(p.authorization.sender, reporter);
    for (address, escrow) in escrows {
        state.accounts.insert(address, escrow);
    }
    if fraud.confiscated != 0 {
        state.accounts.insert(forfeiture_address(), sink);
    }
    state.node_bond_registry = Some(bonds);
    state.node_fraud = Some(fraud);
    Ok(())
}

pub fn validate_block_inputs(
    block: &crate::blockchain::block::Block,
    transitions: &[StateTransition],
    active: bool,
) -> Result<(), StateError> {
    let carried: Vec<_> = block
        .transactions
        .iter()
        .filter(|tx| tx.tx_type == TransactionType::NodeFraudReport)
        .collect();
    let applied: Vec<_> = transitions
        .iter()
        .filter_map(|t| match t {
            StateTransition::ApplyNodeFraudReport {
                payload,
                block_height,
            } => Some((payload, *block_height)),
            _ => None,
        })
        .collect();
    if (!active && (!carried.is_empty() || !applied.is_empty()))
        || carried.len() > MAX_NODE_FRAUD_REPORTS_PER_BLOCK as usize
        || carried.len() != applied.len()
    {
        return Err(refused());
    }
    for (tx, (p, height)) in carried.into_iter().zip(applied) {
        if height != block.height || parse(tx).as_ref() != Some(p) {
            return Err(refused());
        }
    }
    Ok(())
}
