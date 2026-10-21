//! #3301 F2 (review): the checkpointed / snapshot-synced boot shape, in its
//! OWN process so the order is observable (the feature schedule is a
//! process-wide first-install-wins global).
//!
//! Shape: a checkpoint at height 2 > 0 (below the activation), then a WAL
//! tail 3..=6 that crosses the activation at 3 and carries a paymaster
//! `Stake` at 4. The durable lowest block is block 1, carrying the schedule.
//! - Load before the gate: the tail replays with every feature off, the
//!   Stake is refused, the load fails.
//! - `gate_then_load_state`: the gate resolves from durable block 1, the
//!   load anchors on the checkpoint, replays the tail under the schedule and
//!   reproduces the producer's tip state_root.

use std::path::PathBuf;

use wrfcoin_native_chain::storage::{gate_then_load_state, Storage};
use wrfcoin_shared_protocol::accounts::paymaster::PaymasterAction;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule, ConsensusFeature, SigningProtocolVersion,
    GENESIS_ACTIVATION_SCHEDULE_KEY,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;

const ACTIVATION: u64 = 3;
const CHECKPOINT: u64 = 2;
const TIP: u64 = 6;
const STAKE_AT: u64 = 4;
const PAYMASTER: Address = Address([0x2b; 20]);

fn schedule_raw() -> String {
    format!("0:v1,{ACTIVATION}:{}", ConsensusFeature::Paymaster.name())
}

fn genesis_state() -> State {
    let mut state = State::new();
    state.create_account(PAYMASTER, Balance::new(10_000));
    state.recalculate_state_root();
    state
}

fn transitions_at(height: u64) -> Vec<StateTransition> {
    if height == STAKE_AT {
        vec![StateTransition::ApplyPaymasterAction {
            action: PaymasterAction::Stake {
                paymaster: PAYMASTER,
                amount: 1_000,
                unstake_delay_blocks: 2,
            },
            block_height: height,
        }]
    } else {
        Vec::new()
    }
}

fn install_gate_from_block1(storage: &Storage) -> Result<(), String> {
    let block1 = storage
        .load_lowest_block()
        .map_err(|e| e.to_string())?
        .ok_or("no block 1")?;
    assert_eq!(block1.height, 1, "the gate's source is the durable block 1");
    let pairs: Vec<(&str, &str)> = block1
        .metadata
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
        .map_err(|e| e.to_string())?;
    let features = resolve_feature_schedule(pairs, &table).map_err(|e| e.to_string())?;
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    Ok(())
}

/// Block `height`, chained on `prev`; block 1 carries the schedule.
fn next_block(height: u64, prev: &mut Hash) -> Block {
    let mut block = Block::new(1, prev.clone(), vec![], 1, height);
    if height == 1 {
        block
            .metadata
            .insert(GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(), schedule_raw());
    }
    let hash = block.calculate_hash();
    block.set_hash(hash.clone());
    *prev = hash;
    block
}

#[tokio::test]
async fn a_checkpointed_node_replays_its_tail_across_the_activation_under_the_schedule() {
    assert!(try_feature_schedule().is_none(), "this file owns its process");
    let dir = tempfile::tempdir().expect("tempdir");
    let path: PathBuf = dir.path().join("wrfcoin.redb");

    // Persist: heights 1..=CHECKPOINT with a checkpoint at CHECKPOINT (those
    // heights need no feature, so the checkpoint state is computable before
    // anything is installed), then reopen with a long interval so the tail
    // CHECKPOINT+1..=TIP lives only in the WAL.
    let mut prev = Hash::default();
    {
        let mut storage = Storage::open(&path, CHECKPOINT).expect("open");
        storage.persist_genesis_state(&genesis_state()).await.expect("anchor");
        let mut state = genesis_state();
        for height in 1..=CHECKPOINT {
            state
                .apply_transitions_batch_at_height(height, transitions_at(height))
                .expect("pre-activation heights apply with no schedule");
            state.apply_block_finalization(height);
            let checkpoint = storage.checkpoint_state_bytes(height, &state).expect("encode");
            let block = next_block(height, &mut prev);
            storage
                .persist_block(block, transitions_at(height), checkpoint)
                .await
                .expect("persist");
        }
    }
    {
        let mut storage = Storage::open(&path, 100).expect("reopen for the WAL tail");
        for height in CHECKPOINT + 1..=TIP {
            let block = next_block(height, &mut prev);
            storage
                .persist_block(block, transitions_at(height), None)
                .await
                .expect("persist tail");
        }
    }

    let storage = Storage::open(&path, CHECKPOINT).expect("reopen");
    // Old order: the tail crosses the activation with every feature off.
    assert!(
        storage.load_state().is_err(),
        "loading before the gate must fail on the replayed post-activation Stake"
    );

    // #3301 order.
    let ((), loaded) = gate_then_load_state(&storage, install_gate_from_block1)
        .unwrap_or_else(|e| panic!("checkpointed gate-then-load must succeed: {e:?}"));
    let chain = loaded.expect("a persisted chain");
    assert_eq!(chain.height, TIP);

    let mut expected = genesis_state();
    for height in 1..=TIP {
        expected
            .apply_transitions_batch_at_height(height, transitions_at(height))
            .expect("the producer applies with the schedule in force");
        expected.apply_block_finalization(height);
    }
    assert_eq!(chain.state.state_root, expected.state_root);
    assert_eq!(
        chain.state.paymasters.get(&PAYMASTER).map(|p| p.deposit),
        Some(1_000)
    );
}
