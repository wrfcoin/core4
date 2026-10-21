#[path = "../../shared-protocol/tests/common/node_receipt_slice3.rs"]
mod fixture;
use fixture::*;
use wrfcoin_native_chain::{
    node_service_archive::{InstalledNodeReceipt, endorse_installed, server_claim},
    storage::Storage,
};
use wrfcoin_shared_protocol::{
    blockchain::{
        block::Block,
        state::StateTransition,
        state_snapshot::{export_state_entries, import_state_entries},
    },
    consensus::{node_receipt_registry::*, node_share_contracts::BondedServiceClaim},
    crypto::{
        hash::Hash,
        signature::{PublicKey, Signature},
        signer::BlockSigner,
    },
};
struct SyntheticSigner(u8);
impl std::fmt::Debug for SyntheticSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("synthetic dedicated Node signer")
    }
}
impl BlockSigner for SyntheticSigner {
    fn provenance(&self) -> &wrfcoin_shared_protocol::crypto::signer::SignerProvenance {
        static PROVENANCE: std::sync::OnceLock<
            wrfcoin_shared_protocol::crypto::signer::SignerProvenance,
        > = std::sync::OnceLock::new();
        PROVENANCE.get_or_init(|| {
            wrfcoin_shared_protocol::crypto::signer::SignerProvenance::EnvKey {
                var_name: "synthetic-test-only".into(),
            }
        })
    }
    fn public_key(&self) -> PublicKey {
        PublicKey::from_private_key(&key(self.0))
    }
    fn sign(&self, bytes: &[u8]) -> anyhow::Result<Signature> {
        Ok(key(self.0).sign(bytes))
    }
}
#[tokio::test]
async fn accepted_receipt_replays_from_actual_wal_and_snapshot_root() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.redb");
    let mut storage = Storage::open(&path, 10000).unwrap();
    let before = state();
    storage.force_checkpoint(&before, H).await.unwrap();
    let c = claim(&before, 1);
    let p = submission(vec![c], 1);
    let tx = transaction(&p);
    let mut block = Block::new(1, Hash::from_bytes([17; 32]), vec![tx], 0, H + 1);
    block.hash = Some(block.calculate_hash());
    let inputs = vec![
        StateTransition::BeginNodeServiceBlock {
            block_height: H + 1,
            parent_hash: [17; 32],
        },
        StateTransition::ApplyNodeServiceBatch {
            payload: p,
            block_height: H + 1,
        },
    ];
    let mut producer = before.clone();
    producer
        .apply_transitions_batch_at_height(H + 1, inputs.clone())
        .unwrap();
    producer.apply_block_finalization(H + 1);
    storage
        .persist_block(block.clone(), inputs.clone(), None)
        .await
        .unwrap();
    drop(storage);
    let storage = Storage::open(&path, 10000).unwrap();
    let recovered = storage.load_state().unwrap().unwrap().state;
    assert_eq!(recovered.state_root, producer.state_root);
    assert_eq!(
        recovered.node_receipt_registry,
        producer.node_receipt_registry
    );
    let entries = export_state_entries(&recovered).unwrap();
    let restored = import_state_entries(
        &wrfcoin_shared_protocol::blockchain::state::State::new(),
        &entries,
    )
    .unwrap();
    assert_eq!(restored.state_root, recovered.state_root);
    let mut forged = inputs.clone();
    if let StateTransition::ApplyNodeServiceBatch { payload, .. } = &mut forged[1] {
        *payload = submission(vec![claim(&before, 2)], 1);
    }
    assert!(
        wrfcoin_shared_protocol::consensus::node_bond_registry::validate_block_inputs(
            &block, &forged, true
        )
        .is_err()
    );
    let bad = dir.path().join("forged.redb");
    let mut storage = Storage::open(&bad, 10000).unwrap();
    storage.force_checkpoint(&before, H).await.unwrap();
    storage.persist_block(block, forged, None).await.unwrap();
    assert!(storage.load_state().is_err());
}
#[test]
fn receiver_and_server_use_distinct_real_signers_and_archive_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive.redb");
    let storage = Storage::open(&path, 10000).unwrap();
    let s = state();
    let c = claim(&s, 1);
    let installed = InstalledNodeReceipt {
        receipt: c.receipt.clone(),
        manifest: c.manifest.clone(),
    };
    let partial = server_claim(
        &s,
        &installed,
        H,
        &identity(),
        &SyntheticSigner(11),
        &storage,
    )
    .unwrap();
    assert!(partial.consumer_signature.is_empty());
    assert!(
        endorse_installed(
            &s,
            &installed,
            partial.clone(),
            H,
            &identity(),
            &SyntheticSigner(12)
        )
        .is_err()
    );
    let complete = endorse_installed(
        &s,
        &installed,
        partial,
        H,
        &identity(),
        &SyntheticSigner(22),
    )
    .unwrap();
    verify_claim(&s, &complete, H, &identity()).unwrap();
    let key = claim_hash(&complete).unwrap();
    let wire = complete.to_wire().unwrap();
    storage.archive_node_receipt(&key, &wire).unwrap();
    storage.archive_node_receipt(&key, &wire).unwrap();
    assert!(storage.archive_node_receipt(&key, b"conflict").is_err());
    drop(storage);
    let reopened = Storage::open(&path, 10000).unwrap();
    let saved = reopened.archived_node_receipt(&key).unwrap().unwrap();
    assert_eq!(BondedServiceClaim::from_wire(&saved).unwrap(), complete);
    assert!(reopened.archived_node_receipt(&[99; 32]).unwrap().is_none());
    assert!(
        reopened
            .archive_node_receipt(
                &[88; 32],
                &vec![0; wrfcoin_native_chain::storage::MAX_NODE_ARCHIVE_RECORD_BYTES + 1]
            )
            .is_err()
    );
}

fn installed_pair() -> (
    wrfcoin_shared_protocol::blockchain::state::State,
    InstalledNodeReceipt,
    InstalledNodeReceipt,
) {
    let s = state();
    let a = claim(&s, 31);
    let mut b = a.clone();
    b.manifest.payload.tip_hash_hex = hex::encode([66; 32]);
    resign_claim(&mut b, 11, 22);
    (
        s,
        InstalledNodeReceipt {
            receipt: a.receipt,
            manifest: a.manifest,
        },
        InstalledNodeReceipt {
            receipt: b.receipt,
            manifest: b.manifest,
        },
    )
}

#[test]
fn server_ticket_second_different_ticket_refused_and_identical_retry_reuses_signature() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(&dir.path().join("signing.redb"), 10000).unwrap();
    let (s, a, b) = installed_pair();
    let first = server_claim(&s, &a, H, &identity(), &SyntheticSigner(11), &storage).unwrap();
    assert!(server_claim(&s, &b, H, &identity(), &SyntheticSigner(11), &storage).is_err());
    let retry = server_claim(&s, &a, H, &identity(), &SyntheticSigner(11), &storage).unwrap();
    assert_eq!(retry, first);
}

#[test]
fn server_ticket_restart_retains_conflict_guard_and_exact_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("signing.redb");
    let storage = Storage::open(&path, 10000).unwrap();
    let (s, a, b) = installed_pair();
    let first = server_claim(&s, &a, H, &identity(), &SyntheticSigner(11), &storage).unwrap();
    drop(storage);
    let storage = Storage::open(&path, 10000).unwrap();
    assert!(server_claim(&s, &b, H, &identity(), &SyntheticSigner(11), &storage).is_err());
    assert_eq!(
        server_claim(&s, &a, H, &identity(), &SyntheticSigner(11), &storage).unwrap(),
        first
    );
}

#[test]
fn consumer_cannot_frame_honest_server_via_reused_ticket_identifier() {
    use wrfcoin_shared_protocol::consensus::{
        node_bond_registry::{NODE_BOND_UNITS, node_bond_escrow_address},
        node_fraud,
    };
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(&dir.path().join("framing.redb"), 10000).unwrap();
    let (mut s, a, b) = installed_pair();
    let first = server_claim(&s, &a, H, &identity(), &SyntheticSigner(11), &storage).unwrap();
    if let Ok(second) = server_claim(&s, &b, H, &identity(), &SyntheticSigner(11), &storage) {
        let mut p = submission(vec![first, second], 1);
        p.authorization.batch_hash = node_fraud::evidence_hash(&p).unwrap();
        p.owner_signature = sign(&p.authorization, 10);
        s.apply_transition(StateTransition::ApplyNodeFraudReport {
            payload: p,
            block_height: H,
        })
        .unwrap();
        assert_eq!(s.accounts[&node_bond_escrow_address(&[1; 32])].balance.0, 0);
        assert_eq!(
            s.accounts[&node_fraud::forfeiture_address()].balance.0,
            u128::from(NODE_BOND_UNITS)
        );
    }
    assert!(
        !s.node_fraud.as_ref().unwrap().jailed.contains(&[1; 32]),
        "honest server was slashed after consumer framing"
    );
}
