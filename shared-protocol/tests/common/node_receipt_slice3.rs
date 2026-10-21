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
#[test]
fn one_real_consumer_is_sufficient_and_no_mint_or_fee_debit_occurs() {
    let mut s = state();
    let c = claim(&s, 1);
    let amount = s.total_minted_supply();
    let owner = s.accounts[&bonds::address(10)].balance;
    apply(&mut s, submission(vec![c.clone()], 1)).unwrap();
    let r = s.node_receipt_registry.as_ref().unwrap();
    assert!(r.has_service(1, &[1; 32]));
    assert_eq!(r.distinct_consumers(1, &[1; 32]).len(), 1);
    assert!(!r.has_service(1, &[2; 32]));
    assert_eq!(s.total_minted_supply(), amount);
    assert_eq!(s.accounts[&bonds::address(10)].balance, owner);
    let before = s.state_root_material();
    assert!(apply(&mut s, submission(vec![c], 2)).is_err());
    assert_eq!(s.state_root_material(), before, "duplicate-only batch cannot consume a nonce or block slot");
    assert_eq!(s.node_receipt_registry.as_ref().unwrap().accepted.len(), 1);
}
#[test]
fn forged_inner_signature_and_wrong_registered_signer_are_refused() {
    let mut s = state();
    let mut c = claim(&s, 1);
    c.receipt.signature[0] ^= 1;
    resign_outer(&mut c, 11, 22);
    assert!(apply(&mut s, submission(vec![c], 1)).is_err());
    let mut c = claim(&s, 1);
    resign_outer(&mut c, 21, 22);
    assert!(apply(&mut s, submission(vec![c], 1)).is_err());
}
#[test]
fn wrong_alias_manifest_and_inclusion_epoch_are_refused_with_valid_signatures() {
    let mut s = state();
    for which in 0..6 {
        let mut c = claim(&s, 1);
        match which {
            0 => c.receipt.receipt.server_id = "wrong-server".into(),
            1 => c.manifest.payload.snapshot_root_hex = hex::encode([6; 32]),
            2 => c.manifest.payload.entry_count = 2,
            3 => c.manifest.payload.height += 1,
            4 => c.ticket.context.epoch_start_parent = [99; 32],
            _ => c.manifest.signer_id = hex::encode(pk(21)),
        }
        resign_claim(&mut c, if which == 5 { 21 } else { 11 }, 22);
        if which == 5 { resign_outer(&mut c, 11, 22); }
        assert!(
            apply(&mut s, submission(vec![c], 1)).is_err(),
            "mutation {which}"
        );
    }
    let c = claim(&s, 1);
    assert!(verify_claim(&s, &c, H + NODE_EPOCH_BLOCKS, &identity()).is_err());
}
#[test]
fn self_same_account_and_rotated_key_cannot_create_consumers() {
    let mut s = state();
    let mut c = claim(&s, 1);
    c.ticket.consumer_id = [1; 32];
    c.receipt.receipt.consumer_pubkey = pk(12);
    resign_claim(&mut c, 11, 12);
    assert!(apply(&mut s, submission(vec![c], 1)).is_err());
    let mut same = s.clone();
    same.node_bond_registry
        .as_mut()
        .unwrap()
        .frozen_epochs
        .get_mut(&1)
        .unwrap()
        .bindings
        .get_mut(&[2; 32])
        .unwrap()
        .owner_account = bonds::address(10);
    let c = claim(&same, 1);
    assert!(apply(&mut same, submission(vec![c], 1)).is_err());
    let mut c = claim(&s, 1);
    c.receipt.receipt.consumer_pubkey = pk(32);
    resign_claim(&mut c, 11, 32);
    assert!(apply(&mut s, submission(vec![c], 1)).is_err());
    let claims=(1..=3).map(|i|claim(&s,i)).collect();
    apply(&mut s,submission(claims,1)).unwrap();
    assert_eq!(
        s.node_receipt_registry
            .as_ref()
            .unwrap()
            .distinct_consumers(1, &[1; 32])
            .len(),
        1
    );
}
#[test]
fn signed_timestamps_are_informational_not_consensus_freshness() {
    for timestamp in [i64::MIN, 0, i64::MAX] {
        let mut s = state();
        let mut c = claim(&s, 1);
        c.receipt.receipt.timestamp = timestamp;
        c.manifest.timestamp = timestamp;
        resign_claim(&mut c, 11, 22);
        apply(&mut s, submission(vec![c], 1)).unwrap();
    }
}
#[test]
fn bounds_and_root_cover_the_durable_claim() {
    let mut s = state();
    let c = claim(&s, 1);
    let p = submission(vec![c], 1);
    let tx = transaction(&p);
    assert!(NodeReceiptSubmission::parse(&tx).is_some());
    let mut huge = tx.clone();
    huge.data = Some("00".repeat(MAX_NODE_BATCH_BYTES + 1));
    assert!(NodeReceiptSubmission::parse(&huge).is_none());
    assert!(NodeReceiptSubmission::from_wire(&u64::MAX.to_le_bytes()).is_err());
    // Valid bounded claim/count and matching sender/nonce; only the transport
    // signature vector exceeds the global byte budget. Parsing must refuse it
    // before later cryptographic authorization checks.
    let mut over = p.clone();
    over.owner_signature = vec![0; MAX_NODE_BATCH_BYTES + 1];
    huge.data = Some(hex::encode(bincode::serialize(&over).unwrap()));
    assert!(NodeReceiptSubmission::parse(&huge).is_none());
    let root = s.state_root_material();
    apply(&mut s, p).unwrap();
    assert_ne!(root, s.state_root_material());
    let mut changed = s.clone();
    changed
        .node_receipt_registry
        .as_mut()
        .unwrap()
        .accepted
        .values_mut()
        .next()
        .unwrap()
        .claim
        .receipt
        .receipt
        .units += 1;
    assert_ne!(changed.state_root_material(), s.state_root_material());
    assert!(
        NodeServiceBatch {
            claims: vec![claim(&s, 1); MAX_NODE_CLAIMS + 1]
        }
        .to_wire()
        .is_err()
    );
    let more = (2..=MAX_NODE_TICKETS_PER_PAIR).map(|i| claim(&s, i as u8)).collect();
    apply(&mut s, submission(more, 2)).unwrap();
    s.apply_transition(StateTransition::BeginNodeServiceBlock { block_height:H+1, parent_hash:[4;32] }).unwrap();
    let c = claim(&s, 99);
    assert!(s.apply_transition(StateTransition::ApplyNodeServiceBatch { payload:submission(vec![c],3), block_height:H+1 }).is_err());
}

#[test]
fn conflicting_ticket_and_owner_cap_are_refused_atomically() {
    let mut s = state();
    let c = claim(&s, 1);
    apply(&mut s, submission(vec![c.clone()], 1)).unwrap();
    let mut conflict = c.clone();
    conflict.manifest.timestamp = 0;
    resign_claim(&mut conflict, 11, 22);
    let root = s.state_root;
    assert!(apply(&mut s, submission(vec![conflict], 2)).is_err());
    assert_eq!(s.state_root, root);
    assert_eq!(s.accounts[&bonds::address(10)].nonce, 2);
    let c2 = claim(&s, 2);
    apply(&mut s, submission(vec![c2], 2)).unwrap();
    let before = s.state_root_material();
    let c3 = claim(&s, 3);
    assert!(apply(&mut s, submission(vec![c3], 3)).is_err());
    assert_eq!(s.state_root_material(), before, "owner cap refusal is atomic");
    assert_eq!(s.node_receipt_registry.as_ref().unwrap().batch_count, 2);

}

#[test]
fn bounded_retained_shape_and_signature_pricing_are_measured() {
    use wrfcoin_shared_protocol::consensus::verify_cost::{VerifyCost, VerifyPricing, minimum_fee_for};
    let state = state();
    let one = transaction(&submission(vec![claim(&state, 1)], 1));
    let eight = transaction(&submission((1..=8).map(|i| claim(&state, i)).collect(), 1));
    // Ordinary input authorization plus owner authorization plus four service
    // signatures per claim (manifest, inner, server and consumer).
    assert_eq!(one.verify_work().ed25519, 6);
    assert_eq!(eight.verify_work().ed25519, 34);
    let pricing = VerifyPricing::default();
    assert!(minimum_fee_for(&eight, &pricing, 1) > minimum_fee_for(&one, &pricing, 1));
    let mut malformed = one.clone();
    malformed.data = Some("not-hex".into());
    assert_eq!(malformed.verify_work().ed25519, 34);

    // Serialization resource fixture only: all maximum-length variable fields
    // are populated; these altered claims are not evidence of valid service.
    let mut c = claim(&state, 1);
    c.receipt.receipt.server_id = "s".repeat(MAX_NODE_ALIAS_BYTES);
    let wire_len = c.to_wire().unwrap().len();
    c.manifest.nonce = "n".repeat(c.manifest.nonce.len() + MAX_NODE_CLAIM_BYTES - wire_len);
    assert_eq!(c.to_wire().unwrap().len(), MAX_NODE_CLAIM_BYTES);
    let mut store = NodeReceiptRegistry::default();
    let mut index = 0u64;
    for epoch in 0..MAX_RETAINED_NODE_EPOCHS {
        for server in 0..MAX_NODE_REGISTRATIONS {
            for consumer in 0..MAX_NODE_REGISTRATIONS {
                if server == consumer { continue; }
                for ticket in 0..MAX_NODE_TICKETS_PER_PAIR {
                    c.ticket.context.epoch = epoch as u64;
                    c.ticket.server_id = [server as u8; 32];
                    c.ticket.consumer_id = [consumer as u8; 32];
                    c.ticket.ticket_id = [ticket as u8; 32];
                    let mut key = [0; 32]; key[..8].copy_from_slice(&index.to_le_bytes());
                    store.accepted.insert(key, AcceptedNodeClaim { included_height: H, claim: c.clone() });
                    index += 1;
                }
            }
        }
    }
    assert_eq!(index, 23_808);
    let encoded = bincode::serialize(&store).unwrap();
    // 32-byte map key + 8-byte inclusion height, plus 32-byte store header
    // (including the empty per-owner batch-count map).
    let bound = 32 + 23_808 * (MAX_NODE_CLAIM_BYTES + 40);
    assert_eq!(encoded.len(), bound);
    let decoded: NodeReceiptRegistry = bincode::deserialize(&encoded).unwrap();
    assert_eq!(decoded, store);
    println!("NODE_RECEIPT_RETAINED_SHAPE records={} claim_bytes={} encoded_bytes={} upper_bound_bytes={}", index, c.to_wire().unwrap().len(), encoded.len(), bound);
}
