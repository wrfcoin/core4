use wrfcoin_shared_protocol::{
    blockchain::{
        snapshot_sync::{SignedSnapshotSyncPayload, SnapshotSyncPayload},
        state::{State, StateTransition},
        transaction::{Transaction, TransactionType},
    },
    consensus::{
        node_bond_registry::*, node_receipt_registry::*, node_service_receipt::*,
        node_share_contracts::*,
    },
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion, protocol_version_at},
        domain_signing::{DomainSeparated, sign_domain_for},
        signature::{PrivateKey, PublicKey},
    },
};
#[path = "node_bond_fixture.rs"]
pub mod bonds;
pub fn identity() -> ChainIdentity {
    ChainIdentity::new("777", "testnet")
}
pub fn key(byte: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[byte; 32]).unwrap()
}
pub fn pk(byte: u8) -> [u8; 32] {
    PublicKey::from_private_key(&key(byte))
        .as_bytes()
        .try_into()
        .unwrap()
}
pub fn sign<T: DomainSeparated>(subject: &T, byte: u8) -> Vec<u8> {
    sign_domain_for(&key(byte), subject, &identity(), SigningProtocolVersion::V2)
        .as_bytes()
        .to_vec()
}
pub const H: u64 = bonds::A + NODE_EPOCH_BLOCKS;
pub fn state() -> State {
    let mut s = bonds::state();
    bonds::apply(
        &mut s,
        bonds::auth(10, 1, 0, NodeBondAction::Deposit, 1),
        bonds::A,
    )
    .unwrap();
    bonds::apply(
        &mut s,
        bonds::auth(20, 2, 0, NodeBondAction::Deposit, 1),
        bonds::A,
    )
    .unwrap();
    bonds::advance(&mut s, H);
    s
}
pub fn resign_claim(claim: &mut BondedServiceClaim, server: u8, consumer: u8) {
    claim.receipt.signature = sign_domain_for(
        &key(consumer),
        &claim.receipt.receipt,
        &identity(),
        protocol_version_at(claim.receipt.receipt.chain_height),
    )
    .as_bytes()
    .to_vec();
    claim.manifest.signature = key(server)
        .sign(
            claim
                .manifest
                .canonical_signing_message_for(&identity())
                .as_bytes(),
        )
        .to_hex();
    resign_outer(claim, server, consumer);
}
pub fn resign_outer(claim: &mut BondedServiceClaim, server: u8, consumer: u8) {
    claim.ticket.signed_receipt_hash = BondedServiceClaim::receipt_hash(&claim.receipt).unwrap();
    claim.ticket.signed_manifest_hash = BondedServiceClaim::manifest_hash(&claim.manifest).unwrap();
    claim.server_signature = sign(&claim.ticket, server);
    claim.consumer_signature = sign(
        &NodeSettlementEndorsement {
            ticket_preimage_hash: node_contract_digest(&claim.ticket, &identity()),
        },
        consumer,
    );
}
pub fn claim(s: &State, ticket: u8) -> BondedServiceClaim {
    let r = s.node_bond_registry.as_ref().unwrap();
    let f = &r.frozen_epochs[&1];
    let manifest = SignedSnapshotSyncPayload {
        signer_id: hex::encode(pk(11)),
        timestamp: i64::MIN,
        nonce: "snapshot".into(),
        signature: String::new(),
        payload: SnapshotSyncPayload {
            height: H,
            state_root_hex: hex::encode([7; 32]),
            snapshot_root_hex: hex::encode([8; 32]),
            entry_count: 1,
            tip_hash_hex: hex::encode([9; 32]),
            finalized_height: H,
        },
    };
    let receipt = SignedNodeServiceReceipt {
        signature: vec![],
        receipt: NodeServiceReceipt {
            service: NodeService::SnapshotJoin,
            server_id: "node-1".into(),
            consumer_pubkey: pk(22),
            chain_height: H,
            subject_hash: snapshot_subject_hash(&manifest.payload.snapshot_root_hex),
            units: 1,
            timestamp: i64::MIN,
            nonce: [ticket; 32],
        },
    };
    let mut c = BondedServiceClaim {
        receipt,
        manifest,
        ticket: NodeServiceTicket {
            context: NodeSettlementContext {
                genesis_hash: r.genesis_hash,
                chain_id: identity().chain_id,
                network_id: identity().network_id,
                epoch: 1,
                roster_sequence: f.roster_sequence,
                roster_hash: frozen_roster_hash(f),
                epoch_start_parent: f.parent_hash,
            },
            server_id: [1; 32],
            consumer_id: [2; 32],
            ticket_id: [ticket; 32],
            signed_receipt_hash: [0; 32],
            signed_manifest_hash: [0; 32],
        },
        server_signature: vec![],
        consumer_signature: vec![],
    };
    resign_claim(&mut c, 11, 22);
    c
}
pub fn submission(claims: Vec<BondedServiceClaim>, nonce: u64) -> NodeReceiptSubmission {
    let batch = NodeServiceBatch { claims };
    let authorization = NodeReceiptSubmissionAuthorization {
        genesis_hash: [7; 32],
        sender: bonds::address(10),
        nonce,
        batch_hash: batch_hash(&batch).unwrap(),
    };
    NodeReceiptSubmission {
        owner_signature: sign(&authorization, 10),
        authorization,
        batch,
        owner_key: pk(10),
    }
}
pub fn transaction(p: &NodeReceiptSubmission) -> Transaction {
    use wrfcoin_shared_protocol::blockchain::transaction::{TransactionInput, TransactionOutput};
    let mut tx = Transaction::new(
        vec![TransactionInput { txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"synthetic-node-receipt-input"), vout: 0, signature: None, public_key: Some(hex::encode(p.owner_key)), coinbase_data: None }],
        vec![TransactionOutput { address: p.authorization.sender.to_string(), amount: 0, script: "OP_TRUE".into(), nft_data: None }],
    );
    tx.tx_type = TransactionType::NodeServiceBatch;
    tx.sender_address = p.authorization.sender;
    tx.nonce = p.authorization.nonce;
    tx.fee = 100_000;
    tx.data = Some(hex::encode(p.to_wire().unwrap()));
    tx.set_id();
    tx
}
pub fn apply(
    s: &mut State,
    p: NodeReceiptSubmission,
) -> Result<(), wrfcoin_shared_protocol::blockchain::state::StateError> {
    s.apply_transition(StateTransition::ApplyNodeServiceBatch {
        payload: p,
        block_height: H,
    })
}
