//! Real redb checkpoint and WAL replay for the unscheduled Node S2 state.
#[path = "../../shared-protocol/tests/common/node_bond_slice2.rs"]
mod fixture;
use fixture::*;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::{
    blockchain::{block::Block, state::StateTransition},
    consensus::{node_bond_registry::*, node_share_contracts::*},
    crypto::hash::Hash,
};

#[tokio::test]
async fn node_bond_checkpoint_and_wal_rederive_epoch_parent_and_exit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node-bond.redb");
    let mut storage = Storage::open(&path, 10_000).unwrap();
    let mut before = state();
    apply(&mut before, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let epoch_start = A + NODE_EPOCH_BLOCKS;
    advance(&mut before, epoch_start - 1);
    storage
        .force_checkpoint(&before, epoch_start - 1)
        .await
        .unwrap();
    let (height, checkpoint) = storage
        .load_checkpoint_at_or_before(epoch_start - 1)
        .unwrap()
        .unwrap();
    assert_eq!(height, epoch_start - 1);
    assert_eq!(checkpoint.state_root, before.state_root);
    let payload = auth(10, 1, 1, NodeBondAction::Exit, 2);
    let tx = transaction(&payload, 777);
    let mut block = Block::new(1, Hash::from_bytes([17; 32]), vec![tx], 0, epoch_start);
    block.timestamp = chrono::DateTime::from_timestamp(1_790_000_000, 0).unwrap();
    block.hash = Some(block.calculate_hash());
    let inputs = vec![
        StateTransition::BeginNodeServiceBlock {
            block_height: epoch_start,
            parent_hash: [17; 32],
        },
        StateTransition::ApplyNodeBondLifecycle {
            payload,
            block_height: epoch_start,
        },
    ];
    validate_block_inputs(&block, &inputs, true).unwrap();
    let mut producer = before.clone();
    producer
        .apply_transitions_batch_at_height(epoch_start, inputs.clone())
        .unwrap();
    producer.apply_block_finalization(epoch_start);
    let principal_before = before.total_minted_supply();
    assert_eq!(
        producer.total_minted_supply(),
        principal_before,
        "no fee debit or burn"
    );
    storage
        .persist_block(block.clone(), inputs.clone(), None)
        .await
        .unwrap();
    drop(storage);
    let storage = Storage::open(&path, 10_000).unwrap();
    let replayed = storage.load_state().unwrap().unwrap();
    assert_eq!(replayed.state.state_root, producer.state_root);
    assert_eq!(
        replayed.state.node_bond_registry,
        producer.node_bond_registry
    );
    assert_eq!(
        replayed
            .state
            .node_bond_registry
            .as_ref()
            .unwrap()
            .frozen_epochs[&1]
            .parent_hash,
        [17; 32]
    );
    let mut wrong = inputs.clone();
    wrong[0] = StateTransition::BeginNodeServiceBlock {
        block_height: epoch_start,
        parent_hash: [18; 32],
    };
    assert!(validate_block_inputs(&block, &wrong, true).is_err());
    assert!(validate_block_inputs(&block, &inputs[1..], true).is_err());
    let mut corrupt = Storage::open(&dir.path().join("wrong-parent.redb"), 10_000).unwrap();
    corrupt
        .force_checkpoint(&before, epoch_start - 1)
        .await
        .unwrap();
    corrupt.persist_block(block, wrong, None).await.unwrap();
    assert!(
        corrupt.load_state().is_err(),
        "WAL must verify epoch context against its archived block"
    );
}

#[tokio::test]
async fn node_bond_checkpoint_missing_post_activation_marker_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.redb");
    let storage = Storage::open(&path, 1).unwrap();
    let mut old = state();
    old.node_bond_registry = None;
    old.node_receipt_registry = None;
    old.recalculate_state_root();
    let bytes = storage.checkpoint_state_bytes(A, &old).unwrap().unwrap();
    drop(storage);
    // Inject historical-format bytes directly: the current writer refuses this
    // invalid post-activation state before it reaches disk.
    let db = redb::Database::create(&path).unwrap();
    let write = db.begin_write().unwrap();
    {
        let mut checkpoints = write
            .open_table(redb::TableDefinition::<u64, &[u8]>::new("checkpoints"))
            .unwrap();
        checkpoints.insert(A, bytes.as_slice()).unwrap();
    }
    write.commit().unwrap();
    drop(db);
    let storage = Storage::open(&path, 1).unwrap();
    assert!(
        storage.load_checkpoint_at_or_before(A).unwrap().is_none(),
        "old codec must not fabricate a post-A marker"
    );
}
