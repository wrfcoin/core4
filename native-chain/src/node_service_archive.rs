//! Public signed evidence only. Receiver installation evidence is written by
//! the ingest route after atomic installation, before it returns a receipt.
use serde::{Deserialize, Serialize};
use wrfcoin_shared_protocol::{
    blockchain::{snapshot_sync::SignedSnapshotSyncPayload, state::State},
    consensus::{
        node_bond_registry::epoch_at, node_receipt_registry::*,
        node_service_receipt::SignedNodeServiceReceipt, node_share_contracts::*,
    },
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion},
        domain_signing::DomainSeparated,
        signer::BlockSigner,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledNodeReceipt {
    pub receipt: SignedNodeServiceReceipt,
    pub manifest: SignedSnapshotSyncPayload,
}

pub fn server_claim(
    state: &State,
    installed: &InstalledNodeReceipt,
    height: u64,
    identity: &ChainIdentity,
    signer: &dyn BlockSigner,
    storage: &crate::storage::Storage,
) -> anyhow::Result<BondedServiceClaim> {
    let registry = state
        .node_bond_registry
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node service inactive"))?;
    let epoch = epoch_at(registry.activation_height, height)
        .ok_or_else(|| anyhow::anyhow!("Node service inactive"))?;
    let frozen = registry
        .frozen_epochs
        .get(&epoch)
        .ok_or_else(|| anyhow::anyhow!("Node roster unavailable"))?;
    let public = signer.public_key();
    let server = frozen
        .bindings
        .values()
        .find(|b| {
            b.server_key.as_slice() == public.as_bytes()
                && b.service_alias == installed.receipt.receipt.server_id
        })
        .ok_or_else(|| anyhow::anyhow!("registered server signer unavailable"))?;
    let consumer = frozen
        .bindings
        .values()
        .find(|b| b.consumer_key == installed.receipt.receipt.consumer_pubkey)
        .ok_or_else(|| anyhow::anyhow!("registered receiver unavailable"))?;
    let ticket = NodeServiceTicket {
        context: NodeSettlementContext {
            genesis_hash: registry.genesis_hash,
            chain_id: identity.chain_id.clone(),
            network_id: identity.network_id.clone(),
            epoch,
            roster_sequence: frozen.roster_sequence,
            roster_hash: frozen_roster_hash(frozen),
            epoch_start_parent: frozen.parent_hash,
        },
        server_id: server.deployment_id,
        consumer_id: consumer.deployment_id,
        ticket_id: installed.receipt.receipt.nonce,
        signed_receipt_hash: BondedServiceClaim::receipt_hash(&installed.receipt)?,
        signed_manifest_hash: BondedServiceClaim::manifest_hash(&installed.manifest)?,
    };
    let mut claim = BondedServiceClaim {
        receipt: installed.receipt.clone(),
        manifest: installed.manifest.clone(),
        ticket,
        server_signature: vec![],
        consumer_signature: vec![],
    };
    let deadline = wrfcoin_shared_protocol::consensus::node_bond_registry::epoch_close(
        registry.activation_height,
        epoch,
    )
    .and_then(|h| h.checked_add(node_challenge_blocks(state.genesis_emission_chain_id())))
    .ok_or_else(|| anyhow::anyhow!("Node signing deadline overflow"))?;
    let mut key = [0; 72];
    key[..8].copy_from_slice(&epoch.to_le_bytes());
    key[8..40].copy_from_slice(&claim.ticket.consumer_id);
    key[40..].copy_from_slice(&claim.ticket.ticket_id);
    // Ticket signing and fraud verification MUST stay in lockstep: validate
    // the receipt and manifest before persisting/exposing an outer signature,
    // and never sign conflicting work for a consumer-chosen ticket identifier.
    claim.server_signature = storage.protected_node_ticket_signature(
        &key,
        node_contract_digest(&claim.ticket, identity),
        height,
        deadline,
        || {
            let signature = signer
                .sign_at_height(
                    &claim
                        .ticket
                        .signing_preimage_for(identity, SigningProtocolVersion::V2),
                    height,
                )?
                .as_bytes()
                .to_vec();
            let mut candidate = claim.clone();
            candidate.server_signature = signature.clone();
            verify_claim_material(state, &candidate, height, identity)?;
            Ok(signature)
        },
    )?;
    verify_claim_material(state, &claim, height, identity)?;
    Ok(claim)
}

/// Only the route that can load the post-install journal calls this helper.
/// A dry run has neither a legacy receipt nor that durable journal entry.
pub fn endorse_installed(
    state: &State,
    installed: &InstalledNodeReceipt,
    mut claim: BondedServiceClaim,
    height: u64,
    identity: &ChainIdentity,
    signer: &dyn BlockSigner,
) -> anyhow::Result<BondedServiceClaim> {
    anyhow::ensure!(
        claim.receipt == installed.receipt && claim.manifest == installed.manifest,
        "claim does not match durable installation"
    );
    anyhow::ensure!(
        claim.receipt.receipt.consumer_pubkey.as_slice() == signer.public_key().as_bytes(),
        "wrong receiver signer"
    );
    verify_claim_material(state, &claim, height, identity)?;
    let subject = NodeSettlementEndorsement {
        ticket_preimage_hash: node_contract_digest(&claim.ticket, identity),
    };
    claim.consumer_signature = signer
        .sign_at_height(
            &subject.signing_preimage_for(identity, SigningProtocolVersion::V2),
            height,
        )?
        .as_bytes()
        .to_vec();
    verify_claim(state, &claim, height, identity)?;
    Ok(claim)
}
