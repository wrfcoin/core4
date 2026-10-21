//! #3361 option A (F1): producer / P2P applier / WAL replay parity for the
//! faucet sink binding across its activation, at `H_F − 1`, `H_F` and
//! `H_F + 1`, on the live chain's shape (chain 777, no genesis faucet).
//!
//! - **producer**: the real `ProofOfPublishing::create_next_block`;
//! - **applier**: `extract_transitions` → `apply_transitions_batch_at_height`,
//!   the P2P path, block by block from the pre-block state;
//! - **WAL**: every block persisted through `Storage::persist_block`, then the
//!   stored transitions replayed from genesis.
//!
//! All three must reach the same `state_root`, emission ledger and faucet
//! balance at every height; the faucet must be unpaid at `H_F − 1` and paid
//! exactly its slice at `H_F` and `H_F + 1`.
//!
//! The binding is scheduled through the process-wide extension overlay,
//! installed the way the node does from an applied extension
//! (`refresh_feature_schedule_extensions`). The included extractor module
//! carries its own unit tests, which share this binary, so the scenario runs
//! alone in a child process and the parent never touches the overlay.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;

mod helpers {
    pub(crate) mod test_env {
        // Mount the canonical test-only lock and fixture pins (#3521).
        include!("../src/bin/launch_api/test_env.rs");
    }
}

use std::sync::Arc;

use tokio::sync::RwLock;
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::consensus::proof_of_publishing::{
    EmissionPartition, EmissionRole, ProofOfPublishing,
};
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    GENESIS_CHAIN_ID_KEY, refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;

const LIVE_FAUCET: &str = "0xf72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"; // pragma: allowlist secret -- public faucet wallet ADDRESS (not key material), ruled on core4#3361
const H_F: u64 = 3;
const CHILD_ENV: &str = "WRFCOIN_FAUCET_SINK_PARITY_CHILD_3361";
const TEST_NAME: &str = "faucet_sink_binding_producer_applier_wal_agree_across_activation";

fn faucet_balance(state: &State) -> u128 {
    let live = Address::from_hex(LIVE_FAUCET).unwrap();
    state.get_account(&live).map(|a| a.balance.into_inner()).unwrap_or(0)
}

#[tokio::test]
async fn faucet_sink_binding_producer_applier_wal_agree_across_activation() {
    if std::env::var_os(CHILD_ENV).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_ENV, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.contains("1 passed"), "the child must run exactly this test: {stdout}");
        return;
    }

    refresh_feature_schedule_extensions(vec![("faucet_sink_binding".to_string(), H_F)]);

    // Block 1: chain 777 with NO faucet_address — the live genesis's shape.
    let mut genesis = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    genesis.metadata.insert(GENESIS_CHAIN_ID_KEY.into(), "777".into());
    let genesis_transitions = extract_transitions(&State::new(), &genesis, 1, 0);
    assert_eq!(genesis_transitions.len(), 1, "the genesis config must reach the WAL");
    let mut applier = State::new();
    applier
        .apply_transitions_batch_at_height(1, genesis_transitions.clone())
        .unwrap();
    assert_eq!(
        applier.genesis_emission_config.as_ref().unwrap().faucet_address,
        None,
        "the live chain's genesis records no faucet"
    );

    let chain = Arc::new(RwLock::new(Blockchain::default()));
    {
        let mut c = chain.write().await;
        c.height = 1;
        c.state = applier.clone();
    }
    let db = std::env::temp_dir().join(format!("faucet-sink-parity-3361-{}.redb", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let mut storage = Storage::open(&db, DEFAULT_CHECKPOINT_INTERVAL).unwrap();
    storage.persist_block(genesis, genesis_transitions, None).await.unwrap();

    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let miner = Address([0x41; 20]).to_string();
    let mut expected_faucet = 0u128;
    for height in [H_F - 1, H_F, H_F + 1] {
        let before = applier.clone();
        let block = pop
            .create_next_block(chain.clone(), mempool.clone(), &miner)
            .await
            .expect("the real producer builds the block");
        assert_eq!(block.height, height);
        let producer = chain.read().await.state.clone();

        let transitions = extract_transitions(&before, &block, block.height, 0);
        assert_eq!(
            transitions
                .iter()
                .filter(|t| matches!(t, StateTransition::ApplyFaucetReward))
                .count(),
            1,
            "one faucet transition per block at {height}"
        );
        applier
            .apply_transitions_batch_at_height(block.height, transitions.clone())
            .unwrap();
        storage.persist_block(block, transitions, None).await.unwrap();

        let slice = EmissionPartition::at_height(height, Some("777")).slice(EmissionRole::Faucet);
        assert!(slice > 0);
        let paid = if height >= H_F { slice } else { 0 };
        expected_faucet += paid;
        assert_eq!(
            producer.emission_ledger.spent(EmissionRole::Faucet),
            paid,
            "producer faucet spend at {height}"
        );
        assert_eq!(faucet_balance(&producer), expected_faucet, "producer faucet at {height}");
        assert_eq!(applier.state_root, producer.state_root, "applier root at {height}");
        assert_eq!(applier.emission_ledger, producer.emission_ledger, "applier ledger at {height}");
        assert_eq!(faucet_balance(&applier), expected_faucet, "applier faucet at {height}");
    }
    let producer = chain.read().await.state.clone();

    // WAL: replay every stored block from genesis.
    let rows = storage.load_wal_transitions_range(1, H_F + 1).unwrap();
    let mut wal = State::new();
    for height in 1..=H_F + 1 {
        wal.apply_transitions_batch_at_height(
            height,
            rows.iter().filter(|(h, _)| *h == height).map(|(_, t)| t.clone()),
        )
        .unwrap();
    }
    assert_eq!(wal.state_root, producer.state_root, "WAL replay root");
    assert_eq!(wal.emission_ledger, producer.emission_ledger, "WAL replay ledger");
    assert_eq!(faucet_balance(&wal), expected_faucet, "WAL replay faucet");
    assert_eq!(
        wal.genesis_emission_config, producer.genesis_emission_config,
        "the genesis config is never rewritten by the binding"
    );

    drop(storage);
    std::fs::remove_file(db).unwrap();
}
