//! Bonded Node service signed contracts (#2848). Signature validity is not
//! eligibility, actual delivery, historical-root validity or mint/slash authority.
//! S2 lifecycle execution resolves owners, funding and registry bindings in
//! `node_bond_registry`; receipt admission and settlement remain later slices.
//! Outer signatures use V2 framing even when the preserved inner receipt is V1.

use bincode::Options;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::node_service_receipt::SignedNodeServiceReceipt;
use crate::blockchain::snapshot_sync::SignedSnapshotSyncPayload;
use crate::common::types::Address;
use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use crate::crypto::domain_impls::BodyBuilder;
use crate::crypto::domain_signing::{DomainSeparated, verify_domain_for};
use crate::crypto::hash::hash_bytes;
use crate::crypto::signature::{PublicKey, Signature};

pub const NODE_BOND_WRF: u64 = 250;
pub const NODE_EPOCH_BLOCKS: u64 = 1_024;
pub const TESTNET_CHALLENGE_BLOCKS: u64 = 1_024;
/// Owner-ratified 2026-10-11. Added after the inclusive service-epoch
/// challenge deadline; it is never measured from the exit height alone.
pub const NODE_BOND_WITHDRAWAL_BUFFER_BLOCKS: u64 = 100;
pub const MAINNET_CHALLENGE_BLOCKS: u64 = 4_096;

/// Use the same recorded-genesis chain classification as the emission schedule.
/// This reads neither deployment configuration nor process signing identity.
/// Installed only from canonical genesis metadata; no operator/runtime dial.
pub fn node_epoch_blocks() -> u64 {
    crate::governance::schedule_extension::schedule_extension_authority()
        .and_then(|a| a.node_timing()).map_or(NODE_EPOCH_BLOCKS, |t| t.0)
}

pub fn node_challenge_blocks(genesis_chain_id: Option<&str>) -> u64 {
    if let Some(authority) = crate::governance::schedule_extension::schedule_extension_authority() {
        if genesis_chain_id == Some(authority.chain_id()) {
            if let Some((_, challenge)) = authority.node_timing() { return challenge; }
            if authority.network_id() == "testnet" { return TESTNET_CHALLENGE_BLOCKS; }
        }
    }
    use super::proof_of_publishing::{emission_share_bps, TESTNET_EMISSION_SHARE_BPS};
    if emission_share_bps(genesis_chain_id) == &TESTNET_EMISSION_SHARE_BPS {
        TESTNET_CHALLENGE_BLOCKS
    } else {
        MAINNET_CHALLENGE_BLOCKS
    }
}

pub const MIN_DISTINCT_CONSUMERS: usize = 1;
pub const MAX_NODE_REGISTRATIONS: usize = 32;
pub const MAX_NODE_BATCH_BYTES: usize = 16 * 1_024;
pub const MAX_NODE_CLAIMS: usize = 8;
pub const MAX_NODE_CLAIM_BYTES: usize = 1_536;
pub const MAX_NODE_ALIAS_BYTES: usize = 128;
pub const MAX_NODE_FRAUD_BYTES: usize = 8 * 1_024;
pub const MAX_NODE_REGISTRY_BYTES: usize = 16 * 1_024;

/// All outer service signatures bind this exact chain instance, epoch and
/// frozen roster. Parent is the canonical A+eL-1 block hash, supplied by the
/// future common block context, never a locally queried or invented header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSettlementContext {
    pub genesis_hash: [u8; 32],
    pub chain_id: String,
    pub network_id: String,
    pub epoch: u64,
    pub roster_sequence: u64,
    pub roster_hash: [u8; 32],
    pub epoch_start_parent: [u8; 32],
}

impl NodeSettlementContext {
    fn body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .bytes_field(&self.genesis_hash)
            .str_field(&self.chain_id)
            .str_field(&self.network_id)
            .u64_field(self.epoch)
            .u64_field(self.roster_sequence)
            .bytes_field(&self.roster_hash)
            .bytes_field(&self.epoch_start_parent)
            .finish()
    }

    fn matches(&self, identity: &ChainIdentity, genesis: &[u8; 32]) -> bool {
        self.genesis_hash == *genesis
            && self.chain_id == identity.chain_id
            && self.network_id == identity.network_id
    }
}

/// Server endorsement: one ticket authorizes one exact manifest and one full
/// signed v1 receipt, including the inner consumer signature. Multiple tickets
/// for legitimate work are allowed; this contract alone does not deduplicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeServiceTicket {
    pub context: NodeSettlementContext,
    pub server_id: [u8; 32],
    pub consumer_id: [u8; 32],
    pub ticket_id: [u8; 32],
    pub signed_receipt_hash: [u8; 32],
    pub signed_manifest_hash: [u8; 32],
}

impl DomainSeparated for NodeServiceTicket {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.node_service_ticket.v1";
    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .bytes_field(&self.context.body())
            .bytes_field(&self.server_id)
            .bytes_field(&self.consumer_id)
            .bytes_field(&self.ticket_id)
            .bytes_field(&self.signed_receipt_hash)
            .bytes_field(&self.signed_manifest_hash)
            .finish()
    }
}

/// Consumer settlement endorsement commits to the complete server ticket.
/// Its V2 preimage hash transitively binds genesis, roster, parent, parties,
/// ticket ID, exact signed receipt and manifest under the server domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSettlementEndorsement {
    pub ticket_preimage_hash: [u8; 32],
}

impl DomainSeparated for NodeSettlementEndorsement {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.node_settlement_endorsement.v1";
    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .bytes_field(&self.ticket_preimage_hash)
            .finish()
    }
}

/// Owner-only lifecycle intent; explicit ordinal is part of the wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeBondAction {
    Deposit,
    Update,
    Exit,
    Withdraw,
}

/// No balance movement follows from constructing or verifying this subject.
/// Amount is in base units; the future executor must enforce the 250 WRF floor
/// using the chain's unit scale, escrow restrictions and a persistent nonce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeBondAuthorization {
    pub genesis_hash: [u8; 32],
    pub deployment_id: [u8; 32],
    pub owner_account: Address,
    pub escrow_account: Address,
    pub payout_account: Address,
    pub server_key: [u8; 32],
    pub consumer_key: [u8; 32],
    pub service_alias: String,
    pub action: NodeBondAction,
    pub amount_units: u64,
    pub expected_nonce: u64,
    pub effective_epoch: u64,
}

impl DomainSeparated for NodeBondAuthorization {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.node_bond_authorization.v1";
    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .bytes_field(&self.genesis_hash)
            .bytes_field(&self.deployment_id)
            .bytes_field(&self.owner_account.0)
            .bytes_field(&self.escrow_account.0)
            .bytes_field(&self.payout_account.0)
            .bytes_field(&self.server_key)
            .bytes_field(&self.consumer_key)
            .str_field(&self.service_alias)
            .u32_field(self.action as u32)
            .u64_field(self.amount_units)
            .u64_field(self.expected_nonce)
            .u64_field(self.effective_epoch)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeBondKeyRole {
    Server,
    Consumer,
}

/// Dedicated online key possession over the exact owner's authorization,
/// separated by role. It cannot authorize an exit or spend the owner's bond.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeBondPossession {
    pub authorization_preimage_hash: [u8; 32],
    pub role: NodeBondKeyRole,
}

impl DomainSeparated for NodeBondPossession {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.node_bond_possession.v1";
    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .bytes_field(&self.authorization_preimage_hash)
            .u32_field(self.role as u32)
            .finish()
    }
}

/// Signature container; subjects above have distinct domains. The inner
/// receipt and snapshot export retain their existing signing and wire formats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BondedServiceClaim {
    pub receipt: SignedNodeServiceReceipt,
    pub manifest: SignedSnapshotSyncPayload,
    pub ticket: NodeServiceTicket,
    pub server_signature: Vec<u8>,
    pub consumer_signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedNodeBondAuthorization {
    pub authorization: NodeBondAuthorization,
    pub owner_signature: Vec<u8>,
    pub server_signature: Vec<u8>,
    pub consumer_signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeServiceBatch {
    pub claims: Vec<BondedServiceClaim>,
}

impl NodeServiceBatch {
    fn check_claim_bounds(&self) -> Result<(), NodeContractError> {
        if self.claims.is_empty() || self.claims.len() > MAX_NODE_CLAIMS {
            return Err(NodeContractError::Bounds);
        }
        for claim in &self.claims {
            claim.to_wire()?;
        }
        Ok(())
    }

    pub fn to_wire(&self) -> Result<Vec<u8>, NodeContractError> {
        self.check_claim_bounds()?;
        codec()
            .with_limit(MAX_NODE_BATCH_BYTES as u64)
            .serialize(self)
            .map_err(|_| NodeContractError::Bounds)
    }

    pub fn from_wire(bytes: &[u8]) -> Result<Self, NodeContractError> {
        if bytes.len() > MAX_NODE_BATCH_BYTES {
            return Err(NodeContractError::Bounds);
        }
        // Fixed-u64 sequence count is checked before deserializing any claim.
        let count = bytes.get(..8).ok_or(NodeContractError::Bounds)?;
        let count = u64::from_le_bytes(count.try_into().map_err(|_| NodeContractError::Bounds)?);
        if count == 0 || count > MAX_NODE_CLAIMS as u64 {
            return Err(NodeContractError::Bounds);
        }
        let batch: Self = codec()
            .with_limit(MAX_NODE_BATCH_BYTES as u64)
            .deserialize(bytes)
            .map_err(|_| NodeContractError::Bounds)?;
        batch.check_claim_bounds()?;
        Ok(batch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NodeContractError {
    #[error("node contract exceeds encoded bounds or is malformed")]
    Bounds,
    #[error("node contract chain, genesis, identity or digest mismatch")]
    Binding,
    #[error("node contract signature missing, malformed or invalid")]
    Signature,
}

/// Normative wire: bincode fixed integers, LE, no trailing bytes. Not a new
/// transaction tag. Bound *before* decoding to cap hostile allocation/read.
fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .reject_trailing_bytes()
}

pub fn node_contract_digest<T: DomainSeparated>(subject: &T, identity: &ChainIdentity) -> [u8; 32] {
    *hash_bytes(&subject.signing_preimage_for(identity, SigningProtocolVersion::V2)).as_bytes()
}

/// Commitment grammar is frozen in ADR-003; bincode is transport only.
/// Receipt domain + canonical receipt body + exact consumer signature.
pub(crate) fn signed_receipt_hash(receipt: &SignedNodeServiceReceipt) -> [u8; 32] {
    let body = BodyBuilder::new()
        .bytes_field(b"wrfcoin.node_service_receipt.v1")
        .bytes_field(&receipt.receipt.canonical_body())
        .bytes_field(&receipt.signature)
        .finish();
    *hash_bytes(&body).as_bytes()
}

/// Explicit signed-export commitment, independent of serde layout. Every
/// current export field is committed, including tip and finalized height.
pub(crate) fn signed_manifest_hash(manifest: &SignedSnapshotSyncPayload) -> [u8; 32] {
    let p = &manifest.payload;
    let body = BodyBuilder::new()
        .bytes_field(b"wrfcoin.node_snapshot_manifest_commitment.v1")
        .str_field(&manifest.signer_id)
        .i64_field(manifest.timestamp)
        .str_field(&manifest.nonce)
        .u64_field(p.height)
        .str_field(&p.state_root_hex)
        .str_field(&p.snapshot_root_hex)
        .u64_field(p.entry_count)
        .str_field(&p.tip_hash_hex)
        .u64_field(p.finalized_height)
        .str_field(&manifest.signature)
        .finish();
    *hash_bytes(&body).as_bytes()
}

fn verify<T: DomainSeparated>(
    subject: &T,
    key: &[u8; 32],
    sig: &[u8],
    identity: &ChainIdentity,
) -> Result<(), NodeContractError> {
    if sig.len() != 64 {
        return Err(NodeContractError::Signature);
    }
    let key = PublicKey::from_bytes(key).map_err(|_| NodeContractError::Signature)?;
    let sig = Signature::from_bytes(sig).map_err(|_| NodeContractError::Signature)?;
    if !verify_domain_for(&key, subject, &sig, identity, SigningProtocolVersion::V2) {
        return Err(NodeContractError::Signature);
    }
    Ok(())
}

impl BondedServiceClaim {
    /// Allocation-free bounds validation for retained claims. Transport still
    /// uses the frozen codec; this never re-encodes a claim into a fresh Vec.
    pub fn validate_wire_bounds(&self) -> Result<(), NodeContractError> {
        let size = codec().serialized_size(self).map_err(|_| NodeContractError::Bounds)?;
        if size > MAX_NODE_CLAIM_BYTES as u64 { return Err(NodeContractError::Bounds); }
        Ok(())
    }

    pub fn to_wire(&self) -> Result<Vec<u8>, NodeContractError> {
        codec()
            .with_limit(MAX_NODE_CLAIM_BYTES as u64)
            .serialize(self)
            .map_err(|_| NodeContractError::Bounds)
    }

    pub fn from_wire(bytes: &[u8]) -> Result<Self, NodeContractError> {
        if bytes.len() > MAX_NODE_CLAIM_BYTES {
            return Err(NodeContractError::Bounds);
        }
        codec()
            .with_limit(MAX_NODE_CLAIM_BYTES as u64)
            .deserialize(bytes)
            .map_err(|_| NodeContractError::Bounds)
    }

    /// Contract-only verification. Caller supplies already frozen registry keys
    /// and genesis; later S3 must check roster/account aliases, manifest export
    /// signature, epoch heights, service/root/units and chain inclusion. Never
    /// use this result as payment eligibility or objective fraud evidence.
    pub fn verify_server_contract(
        &self,
        identity: &ChainIdentity,
        genesis: &[u8; 32],
        server_key: &[u8; 32],
        consumer_key: &[u8; 32],
        inner_version: SigningProtocolVersion,
    ) -> Result<(), NodeContractError> {
        self.to_wire()?;
        if !self.ticket.context.matches(identity, genesis)
            || self.ticket.server_id == self.ticket.consumer_id
            || server_key == consumer_key
            || self.receipt.receipt.consumer_pubkey != *consumer_key
            || self.receipt.receipt.server_id.is_empty()
            || self.receipt.receipt.server_id.len() > MAX_NODE_ALIAS_BYTES
            || self.ticket.signed_receipt_hash != signed_receipt_hash(&self.receipt)
            || self.ticket.signed_manifest_hash != signed_manifest_hash(&self.manifest)
        {
            return Err(NodeContractError::Binding);
        }
        self.receipt
            .verify_signature(identity, inner_version)
            .map_err(|_| NodeContractError::Signature)?;
        verify(&self.ticket, server_key, &self.server_signature, identity)?;
        Ok(())
    }

    pub fn verify_contract(
        &self,
        identity: &ChainIdentity,
        genesis: &[u8; 32],
        server_key: &[u8; 32],
        consumer_key: &[u8; 32],
        inner_version: SigningProtocolVersion,
    ) -> Result<(), NodeContractError> {
        self.verify_server_contract(identity, genesis, server_key, consumer_key, inner_version)?;
        self.verify_consumer_endorsement(identity, consumer_key)
    }

    /// The caller separately verifies the complete server/material contract.
    pub fn verify_consumer_endorsement(
        &self,
        identity: &ChainIdentity,
        consumer_key: &[u8; 32],
    ) -> Result<(), NodeContractError> {
        verify(
            &NodeSettlementEndorsement {
                ticket_preimage_hash: node_contract_digest(&self.ticket, identity),
            },
            consumer_key,
            &self.consumer_signature,
            identity,
        )
    }

    pub fn receipt_hash(receipt: &SignedNodeServiceReceipt) -> Result<[u8; 32], NodeContractError> {
        if receipt.receipt.server_id.len() > MAX_NODE_ALIAS_BYTES || receipt.signature.len() != 64 {
            return Err(NodeContractError::Bounds);
        }
        Ok(signed_receipt_hash(receipt))
    }

    pub fn manifest_hash(
        manifest: &SignedSnapshotSyncPayload,
    ) -> Result<[u8; 32], NodeContractError> {
        // Preserve the public helper's transport bound before constructing the
        // separate canonical commitment; transport bytes never enter the hash.
        codec()
            .with_limit(MAX_NODE_CLAIM_BYTES as u64)
            .serialize(manifest)
            .map_err(|_| NodeContractError::Bounds)?;
        Ok(signed_manifest_hash(manifest))
    }
}

impl SignedNodeBondAuthorization {
    pub fn to_wire(&self) -> Result<Vec<u8>, NodeContractError> {
        codec()
            .with_limit(MAX_NODE_REGISTRY_BYTES as u64)
            .serialize(self)
            .map_err(|_| NodeContractError::Bounds)
    }

    pub fn from_wire(bytes: &[u8]) -> Result<Self, NodeContractError> {
        if bytes.len() > MAX_NODE_REGISTRY_BYTES {
            return Err(NodeContractError::Bounds);
        }
        codec()
            .with_limit(MAX_NODE_REGISTRY_BYTES as u64)
            .deserialize(bytes)
            .map_err(|_| NodeContractError::Bounds)
    }

    pub fn verify_contract(
        &self,
        identity: &ChainIdentity,
        genesis: &[u8; 32],
        owner_key: &[u8; 32],
    ) -> Result<(), NodeContractError> {
        self.to_wire()?;
        let a = &self.authorization;
        if a.genesis_hash != *genesis
            || a.service_alias.is_empty()
            || a.service_alias.len() > MAX_NODE_ALIAS_BYTES
            || a.server_key == a.consumer_key
            || *owner_key == a.server_key
            || *owner_key == a.consumer_key
            || a.owner_account == a.escrow_account
            || a.payout_account == a.escrow_account
        {
            return Err(NodeContractError::Binding);
        }
        // Owner key -> account binding and cross-registration aliases are S2.
        verify(a, owner_key, &self.owner_signature, identity)?;
        if matches!(a.action, NodeBondAction::Exit | NodeBondAction::Withdraw) {
            // Offline owner can exit/withdraw without online key cooperation.
            // Surplus signatures are refused, keeping a single wire contract.
            return if self.server_signature.is_empty() && self.consumer_signature.is_empty() {
                Ok(())
            } else {
                Err(NodeContractError::Signature)
            };
        }
        let digest = node_contract_digest(a, identity);
        for (role, key, signature) in [
            (
                NodeBondKeyRole::Server,
                &a.server_key,
                &self.server_signature,
            ),
            (
                NodeBondKeyRole::Consumer,
                &a.consumer_key,
                &self.consumer_signature,
            ),
        ] {
            verify(
                &NodeBondPossession {
                    authorization_preimage_hash: digest,
                    role,
                },
                key,
                signature,
                identity,
            )?;
        }
        Ok(())
    }
}
