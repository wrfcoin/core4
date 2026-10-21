//! Identical fixture runs against baseline and candidate; compare output bytes.
use wrfcoin_shared_protocol::{
    blockchain::{block::Block, state::{State, StateTransition}},
    common::types::{Address, Balance},
    consensus::proof_of_publishing::coinbase_allowance,
    crypto::{chain_binding::GenesisEmissionConfig, hash::Hash},
};
#[test]
fn replay_window_777_root_bytes() {
    let mut b = Block::new(3, Hash::default(), vec![], 1, 1);
    b.metadata.insert("chain_id".into(), "777".into());
    b.metadata.insert("network_id".into(), "testnet".into());
    let config = GenesisEmissionConfig::from_block(&b).unwrap();
    let mut state = State::new();
    state.apply_transitions_batch_at_height(1, vec![StateTransition::RecordGenesisEmissionConfig { config }]).unwrap();
    let mut rows = Vec::new();
    for h in (1..=150).chain(49_900..=50_050) {
        let transitions = vec![StateTransition::ApplyReward {
            address: Address([0x25; 20]), amount: Balance::new(coinbase_allowance(h, Some("777"))), reason: "synthetic replay coinbase".into(),
        }, StateTransition::ApplyFaucetReward];
        let wal = serde_json::to_vec(&transitions).unwrap();
        let replay: Vec<StateTransition> = serde_json::from_slice(&wal).unwrap();
        state.apply_transitions_batch_at_height(h, replay).unwrap();
        rows.push(format!("{h} {}", hex::encode(state.state_root.0)));
    }
    if let Ok(path) = std::env::var("WRFCOIN_TEST_REPLAY_OUTPUT") {
        std::fs::write(path, rows.join("\n") + "\n").unwrap();
    }
    assert_eq!(rows.len(), 301);
}
