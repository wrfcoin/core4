//! Height-bound bonded SnapshotJoin inputs; deterministic bounded admission.
use super::{
    node_bond_registry::{NODE_BOND_UNITS, epoch_at},
    node_service_receipt::{NodeService, snapshot_subject_hash},
    node_share_contracts::*,
};
use crate::{
    blockchain::{
        state::{State, StateError},
        transaction::{Transaction, TransactionType},
    },
    common::types::Address,
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion, protocol_version_at},
        domain_impls::BodyBuilder,
        domain_signing::{DomainSeparated, verify_domain_for},
        hash::hash_bytes,
        signature::{PublicKey, Signature},
    },
};
use bincode::Options;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_NODE_RECEIPT_BATCHES_PER_BLOCK: usize = 16;
pub const MAX_NODE_TICKETS_PER_PAIR: usize = 8;

/// Offline spending key authorizes the exact bounded batch and account nonce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeReceiptSubmissionAuthorization {
    pub genesis_hash: [u8; 32],
    pub sender: Address,
    pub nonce: u64,
    pub batch_hash: [u8; 32],
}
impl DomainSeparated for NodeReceiptSubmissionAuthorization {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.node_receipt_submission.v1";
    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .bytes_field(&self.genesis_hash)
            .bytes_field(&self.sender.0)
            .u64_field(self.nonce)
            .bytes_field(&self.batch_hash)
            .finish()
    }
}

/// Bincode is bounded transport only; all signature subjects are explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeReceiptSubmission {
    pub batch: NodeServiceBatch,
    pub authorization: NodeReceiptSubmissionAuthorization,
    pub owner_key: [u8; 32],
    pub owner_signature: Vec<u8>,
}
fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .reject_trailing_bytes()
        .with_limit(MAX_NODE_BATCH_BYTES as u64)
}
fn refused() -> StateError {
    StateError::Governance("SP_NODE_RECEIPT_REFUSED".into())
}

pub fn claim_body(claim: &BondedServiceClaim) -> Result<Vec<u8>, NodeContractError> {
    Ok(BodyBuilder::new()
        .bytes_field(&super::node_share_contracts::signed_receipt_hash(&claim.receipt))
        .bytes_field(&super::node_share_contracts::signed_manifest_hash(&claim.manifest))
        .bytes_field(&claim.ticket.canonical_body())
        .bytes_field(&claim.server_signature)
        .bytes_field(&claim.consumer_signature)
        .finish())
}
pub fn claim_hash(claim: &BondedServiceClaim) -> Result<[u8; 32], NodeContractError> {
    Ok(*hash_bytes(&claim_body(claim)?).as_bytes())
}
pub fn batch_hash(batch: &NodeServiceBatch) -> Result<[u8; 32], NodeContractError> {
    batch.to_wire()?;
    let mut b = BodyBuilder::new()
        .bytes_field(b"wrfcoin.node_receipt_batch.v1")
        .u64_field(batch.claims.len() as u64);
    for claim in &batch.claims {
        b = b.bytes_field(&claim_body(claim)?);
    }
    Ok(*hash_bytes(&b.finish()).as_bytes())
}
impl NodeReceiptSubmission {
    pub fn to_wire(&self) -> Result<Vec<u8>, NodeContractError> {
        self.batch.to_wire()?;
        codec()
            .serialize(self)
            .map_err(|_| NodeContractError::Bounds)
    }
    pub fn from_wire(bytes: &[u8]) -> Result<Self, NodeContractError> {
        if bytes.len() > MAX_NODE_BATCH_BYTES {
            return Err(NodeContractError::Bounds);
        }
        let count = u64::from_le_bytes(
            bytes
                .get(..8)
                .ok_or(NodeContractError::Bounds)?
                .try_into()
                .map_err(|_| NodeContractError::Bounds)?,
        );
        if count == 0 || count > MAX_NODE_CLAIMS as u64 {
            return Err(NodeContractError::Bounds);
        }
        let p: Self = codec()
            .deserialize(bytes)
            .map_err(|_| NodeContractError::Bounds)?;
        p.batch.to_wire()?;
        Ok(p)
    }
    pub fn parse(tx: &Transaction) -> Option<Self> {
        if tx.tx_type != TransactionType::NodeServiceBatch {
            return None;
        }
        let data = tx.data.as_deref()?;
        if data.len() > MAX_NODE_BATCH_BYTES * 2 {
            return None;
        }
        let p = Self::from_wire(&hex::decode(data).ok()?).ok()?;
        (p.authorization.sender == tx.sender_address && p.authorization.nonce == tx.nonce)
            .then_some(p)
    }
    pub fn verify_authorization(
        &self,
        identity: &ChainIdentity,
        genesis: &[u8; 32],
    ) -> Result<(), StateError> {
        self.to_wire().map_err(|_| refused())?;
        let key = PublicKey::from_bytes(&self.owner_key).map_err(|_| refused())?;
        let sig = Signature::from_bytes(&self.owner_signature).map_err(|_| refused())?;
        if self.authorization.genesis_hash != *genesis
            || self.authorization.sender != Address::from_public_key(&key)
            || self.authorization.batch_hash != batch_hash(&self.batch).map_err(|_| refused())?
            || !verify_domain_for(
                &key,
                &self.authorization,
                &sig,
                identity,
                SigningProtocolVersion::V2,
            )
        {
            return Err(refused());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedNodeClaim {
    pub included_height: u64,
    pub claim: BondedServiceClaim,
}
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NodeReceiptRegistry {
    pub block_height: u64,
    pub batch_count: u64,
    pub accepted: BTreeMap<[u8; 32], AcceptedNodeClaim>,
    pub owner_batch_counts: BTreeMap<Address, u64>,
}
fn ticket_key(claim: &BondedServiceClaim) -> [u8; 32] {
    let t = &claim.ticket;
    *hash_bytes(
        &BodyBuilder::new()
            .bytes_field(b"wrfcoin.node_receipt_dedup.v1")
            .u64_field(t.context.epoch)
            .bytes_field(&t.server_id)
            .bytes_field(&t.consumer_id)
            .bytes_field(&t.ticket_id)
            .finish(),
    )
    .as_bytes()
}
impl NodeReceiptRegistry {
    pub fn advance(&mut self, state_registry: &super::node_bond_registry::NodeBondRegistry) {
        if self.block_height != state_registry.block_height {
            self.batch_count = 0;
            self.owner_batch_counts.clear();
        }
        self.block_height = state_registry.block_height;
        self.accepted.retain(|_, record| {
            state_registry
                .frozen_epochs
                .contains_key(&record.claim.ticket.context.epoch)
        });
    }
    pub fn distinct_consumers(&self, epoch: u64, server: &[u8; 32]) -> BTreeSet<[u8; 32]> {
        self.accepted
            .values()
            .filter(|r| {
                r.claim.ticket.context.epoch == epoch && &r.claim.ticket.server_id == server
            })
            .map(|r| r.claim.ticket.consumer_id)
            .collect()
    }
    pub fn has_service(&self, epoch: u64, server: &[u8; 32]) -> bool {
        self.distinct_consumers(epoch, server).len() >= MIN_DISTINCT_CONSUMERS
    }
    pub fn canonical_body(&self) -> Vec<u8> {
        let mut b = BodyBuilder::new()
            .u64_field(self.block_height)
            .u64_field(self.batch_count)
            .u64_field(self.accepted.len() as u64);
        b = b.u64_field(self.owner_batch_counts.len() as u64);
        for (owner, count) in &self.owner_batch_counts { b = b.bytes_field(&owner.0).u64_field(*count); }
        for (id, record) in &self.accepted {
            b = b
                .bytes_field(id)
                .u64_field(record.included_height)
                .bytes_field(&claim_body(&record.claim).expect("admitted bounded claim"));
        }
        b.finish()
    }
}

pub fn frozen_roster_hash(epoch: &super::node_bond_registry::FrozenNodeEpoch) -> [u8; 32] {
    let mut b = BodyBuilder::new()
        .bytes_field(b"wrfcoin.node_frozen_roster.v1")
        .u64_field(epoch.roster_sequence)
        .u64_field(epoch.bindings.len() as u64);
    for (id, binding) in &epoch.bindings {
        b = b.bytes_field(id).bytes_field(&binding.canonical_body());
    }
    *hash_bytes(&b.finish()).as_bytes()
}

/// Consensus does not read the legacy HTTP acknowledgement's local clock.
pub fn verify_claim_material(
    state: &State,
    claim: &BondedServiceClaim,
    height: u64,
    identity: &ChainIdentity,
) -> Result<(), StateError> {
    let registry = state.node_bond_registry.as_ref().ok_or_else(refused)?;
    let epoch = epoch_at(registry.activation_height, height).ok_or_else(refused)?;
    let frozen = registry.frozen_epochs.get(&epoch).ok_or_else(refused)?;
    let t = &claim.ticket;
    let server = frozen.bindings.get(&t.server_id).ok_or_else(refused)?;
    let consumer = frozen.bindings.get(&t.consumer_id).ok_or_else(refused)?;
    let r = &claim.receipt.receipt;
    let m = &claim.manifest;
    if t.context.epoch != epoch
        || t.context.roster_sequence != frozen.roster_sequence
        || t.context.roster_hash != frozen_roster_hash(frozen)
        || t.context.epoch_start_parent != frozen.parent_hash
        || t.server_id == t.consumer_id
        || server.owner_account == consumer.owner_account
        || server.server_key == consumer.consumer_key
        || server.consumer_key == consumer.consumer_key
        || r.server_id != server.service_alias
        || r.service != NodeService::SnapshotJoin
        || epoch_at(registry.activation_height, r.chain_height) != Some(epoch)
        || r.chain_height > height
        || r.chain_height != m.payload.height
        || r.units == 0
        || r.units != m.payload.entry_count
        || r.subject_hash != snapshot_subject_hash(&m.payload.snapshot_root_hex)
        || m.payload.finalized_height > m.payload.height
        || hex::decode(&m.signer_id).ok().as_deref() != Some(server.server_key.as_slice())
        || !m.has_valid_signature_for(identity)
    {
        return Err(refused());
    }
    for hash in [
        &m.payload.snapshot_root_hex,
        &m.payload.state_root_hex,
        &m.payload.tip_hash_hex,
    ] {
        if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(refused());
        }
    }
    for binding in [server, consumer] {
        let record = registry
            .registrations
            .get(&binding.deployment_id)
            .ok_or_else(refused)?;
        let escrow = state
            .accounts
            .get(&binding.escrow_account)
            .ok_or_else(refused)?;
        if record.principal_units != NODE_BOND_UNITS
            || escrow.balance.0 != u128::from(NODE_BOND_UNITS)
            || escrow.stake_locked != escrow.balance
            || escrow.spendable_balance.0 != 0
        {
            return Err(refused());
        }
    }
    claim
        .verify_server_contract(
            identity,
            &registry.genesis_hash,
            &server.server_key,
            &consumer.consumer_key,
            protocol_version_at(r.chain_height),
        )
        .map_err(|_| refused())
}

pub fn verify_claim(
    state: &State,
    claim: &BondedServiceClaim,
    height: u64,
    identity: &ChainIdentity,
) -> Result<(), StateError> {
    verify_claim_material(state, claim, height, identity)?;
    let registry = state.node_bond_registry.as_ref().ok_or_else(refused)?;
    let frozen = &registry.frozen_epochs[&claim.ticket.context.epoch];
    let consumer = &frozen.bindings[&claim.ticket.consumer_id];
    claim
        .verify_consumer_endorsement(identity, &consumer.consumer_key)
        .map_err(|_| refused())
}

pub fn apply_submission(
    state: &mut State,
    p: &NodeReceiptSubmission,
    height: u64,
) -> Result<(), StateError> {
    use crate::accounts::authorization::{
        AuthorizationRule, AuthorizationView, DEBIT_TIME_UNPLUMBED_2168, authorization_for,
    };
    let registry = state.node_bond_registry.as_ref().ok_or_else(refused)?;
    if registry.block_height != height
        || !state
            .node_share_activation_height()
            .is_some_and(|a| height >= a)
    {
        return Err(refused());
    }
    let authority = crate::governance::schedule_extension::schedule_extension_authority()
        .ok_or_else(refused)?;
    let c = authority.context();
    let identity = ChainIdentity::new(c.chain_id, c.network_id);
    p.verify_authorization(&identity, &registry.genesis_hash)?;
    if !matches!(
        authorization_for(
            &state.declared(&p.authorization.sender),
            height,
            DEBIT_TIME_UNPLUMBED_2168
        ),
        AuthorizationRule::SignerIsSender
    ) {
        return Err(refused());
    }
    let mut owner = state
        .accounts
        .get(&p.authorization.sender)
        .cloned()
        .ok_or_else(refused)?;
    if owner.nonce != p.authorization.nonce {
        return Err(refused());
    }
    let receipts = state.node_receipt_registry.as_ref().ok_or_else(refused)?;
    if receipts.block_height != height
        || receipts.batch_count >= MAX_NODE_RECEIPT_BATCHES_PER_BLOCK as u64
    {
        return Err(refused());
    }
    if receipts.owner_batch_counts.get(&p.authorization.sender).copied().unwrap_or(0) >= MAX_NODE_BATCHES_PER_OWNER_PER_BLOCK {
        return Err(refused());
    }
    let epoch = epoch_at(registry.activation_height, height).ok_or_else(refused)?;
    let frozen = registry.frozen_epochs.get(&epoch).ok_or_else(refused)?;
    let mut additions: BTreeMap<[u8; 32], AcceptedNodeClaim> = BTreeMap::new();
    for claim in &p.batch.claims {
        if frozen.bindings.get(&claim.ticket.server_id).map(|b| b.owner_account) != Some(p.authorization.sender) {
            return Err(StateError::Governance("SP_NODE_RECEIPT_OWNER".into()));
        }
        verify_claim(state, claim, height, &identity)?;
        let id = ticket_key(claim);
        if let Some(old) = receipts.accepted.get(&id).or_else(|| additions.get(&id)) {
            if old.claim != *claim {
                return Err(refused());
            }
            continue;
        }
        let pair_count = receipts
            .accepted
            .values()
            .chain(additions.values())
            .filter(|r| {
                r.claim.ticket.context.epoch == epoch
                    && r.claim.ticket.server_id == claim.ticket.server_id
                    && r.claim.ticket.consumer_id == claim.ticket.consumer_id
            })
            .count();
        if pair_count >= MAX_NODE_TICKETS_PER_PAIR {
            return Err(refused());
        }
        additions.insert(
            id,
            AcceptedNodeClaim {
                included_height: height,
                claim: claim.clone(),
            },
        );
    }
    if additions.is_empty() { return Err(refused()); }
    // Validate the complete bounded write set before mutation: neither a
    // rejected claim nor a nonce overflow can leave a partial submission.

    owner.nonce = owner
        .nonce
        .checked_add(1)
        .ok_or(StateError::NonceOverflow)?;
    let receipts = state.node_receipt_registry.as_mut().ok_or_else(refused)?;
    receipts.accepted.extend(additions);
    receipts.batch_count += 1;
    *receipts.owner_batch_counts.entry(p.authorization.sender).or_default() += 1;
    state.accounts.insert(p.authorization.sender, owner);
    Ok(())
}

pub fn validate_receipt_anchor(state: &State, height: u64) -> Result<(), StateError> {
    let registry = state.node_bond_registry.as_ref().ok_or_else(refused)?;
    let receipts = state.node_receipt_registry.as_ref().ok_or_else(refused)?;
    if receipts.block_height != height
        || receipts.batch_count > MAX_NODE_RECEIPT_BATCHES_PER_BLOCK as u64
        || receipts.accepted.len()
            > super::node_bond_registry::max_retained_node_epochs(state.genesis_emission_chain_id())
                * MAX_NODE_REGISTRATIONS
                * (MAX_NODE_REGISTRATIONS - 1)
                * MAX_NODE_TICKETS_PER_PAIR
    {
        return Err(refused());
    }
    if receipts.owner_batch_counts.values().any(|count| *count == 0 || *count > MAX_NODE_BATCHES_PER_OWNER_PER_BLOCK)
        || receipts.owner_batch_counts.values().sum::<u64>() != receipts.batch_count {
        return Err(refused());
    }
    let mut pairs = BTreeMap::new();
    for (key, record) in &receipts.accepted {
        let t = &record.claim.ticket;
        record.claim.validate_wire_bounds().map_err(|_| refused())?;
        if *key != ticket_key(&record.claim)
            || record.included_height > height
            || epoch_at(registry.activation_height, record.included_height) != Some(t.context.epoch)
            || !registry.frozen_epochs.contains_key(&t.context.epoch)
        {
            return Err(refused());
        }
        let count = pairs
            .entry((t.context.epoch, t.server_id, t.consumer_id))
            .or_insert(0usize);
        *count += 1;
        if *count > MAX_NODE_TICKETS_PER_PAIR {
            return Err(refused());
        }
    }
    Ok(())
}

pub fn validate_block_inputs(
    block: &crate::blockchain::block::Block,
    transitions: &[crate::blockchain::state::StateTransition],
    active: bool,
) -> Result<(), StateError> {
    use crate::blockchain::state::StateTransition;
    let carried: Vec<_> = block
        .transactions
        .iter()
        .filter(|tx| tx.tx_type == TransactionType::NodeServiceBatch)
        .collect();
    let applied: Vec<_> = transitions
        .iter()
        .filter_map(|t| match t {
            StateTransition::ApplyNodeServiceBatch {
                payload,
                block_height,
            } => Some((payload, *block_height)),
            _ => None,
        })
        .collect();
    if (!active && (!carried.is_empty() || !applied.is_empty()))
        || carried.len() > MAX_NODE_RECEIPT_BATCHES_PER_BLOCK
        || carried.len() != applied.len()
    {
        return Err(refused());
    }
    for (tx, (p, height)) in carried.into_iter().zip(applied) {
        if height != block.height || NodeReceiptSubmission::parse(tx).as_ref() != Some(p) {
            return Err(refused());
        }
    }
    Ok(())
}

/// Two batches per owner leaves block capacity available to other bonded owners.
pub const MAX_NODE_BATCHES_PER_OWNER_PER_BLOCK: u64 = 2;
