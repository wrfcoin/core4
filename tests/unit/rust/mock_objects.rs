//! Mock objects for Wrfcoin testing framework
//! 
//! This module provides mock implementations of core Wrfcoin components
//! for testing purposes, allowing isolated unit testing without external dependencies.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock blockchain block for testing
#[derive(Debug, Clone)]
pub struct MockBlock {
    pub hash: String,
    pub previous_hash: String,
    pub timestamp: u64,
    pub transactions: Vec<MockTransaction>,
    pub block_type: String,
    pub consensus_type: String,
    pub status: String,
    pub total_fees: u64,
    pub reward: u64,
    pub signature: String,
    pub computation_hash: String,
    pub data_hash: String,
    pub governance_root: String,
    pub computational_metadata: HashMap<String, String>,
    pub cross_chain_metadata: HashMap<String, String>,
    pub data_aware_metadata: HashMap<String, String>,
    pub checkpoint_metadata: HashMap<String, String>,
}

impl Default for MockBlock {
    fn default() -> Self {
        Self {
            hash: "mock_block_hash".to_string(),
            previous_hash: "mock_previous_hash".to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            transactions: vec![],
            block_type: "standard".to_string(),
            consensus_type: "proof_of_work".to_string(),
            status: "confirmed".to_string(),
            total_fees: 0,
            reward: 1000,
            signature: "mock_signature".to_string(),
            computation_hash: "mock_computation_hash".to_string(),
            data_hash: "mock_data_hash".to_string(),
            governance_root: "mock_governance_root".to_string(),
            computational_metadata: HashMap::new(),
            cross_chain_metadata: HashMap::new(),
            data_aware_metadata: HashMap::new(),
            checkpoint_metadata: HashMap::new(),
        }
    }
}

/// Mock transaction for testing
#[derive(Debug, Clone)]
pub struct MockTransaction {
    pub hash: String,
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub fee: u64,
    pub timestamp: u64,
}

impl Default for MockTransaction {
    fn default() -> Self {
        Self {
            hash: "mock_tx_hash".to_string(),
            from: "mock_from_address".to_string(),
            to: "mock_to_address".to_string(),
            amount: 100,
            fee: 1,
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }
}

/// Mock climate data for testing
#[derive(Debug, Clone)]
pub struct MockClimateData {
    pub temperature: f64,
    pub humidity: f64,
    pub pressure: f64,
    pub wind_speed: f64,
    pub location: String,
    pub timestamp: u64,
}

impl Default for MockClimateData {
    fn default() -> Self {
        Self {
            temperature: 22.5,
            humidity: 65.0,
            pressure: 1013.25,
            wind_speed: 5.2,
            location: "test_location".to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }
}

/// Mock cross-chain bridge for testing
pub struct MockCrossChainBridge {
    pub supported_networks: Vec<String>,
    pub messages: Arc<Mutex<Vec<MockCrossChainMessage>>>,
    pub status: String,
}

impl MockCrossChainBridge {
    pub fn new() -> Self {
        Self {
            supported_networks: vec!["ethereum".to_string(), "polygon".to_string()],
            messages: Arc::new(Mutex::new(vec![])),
            status: "active".to_string(),
        }
    }
    
    pub fn send_message(&self, message: MockCrossChainMessage) -> Result<String, String> {
        let mut messages = self.messages.lock().unwrap();
        let message_id = format!("msg_{}", messages.len());
        messages.push(message);
        Ok(message_id)
    }
    
    pub fn get_message_status(&self, _message_id: &str) -> String {
        "confirmed".to_string()
    }
    
    pub fn get_bridge_status(&self) -> String {
        self.status.clone()
    }
    
    pub fn get_supported_networks(&self) -> Vec<String> {
        self.supported_networks.clone()
    }
    
    pub fn supports_route(&self, from: &str, to: &str) -> bool {
        self.supported_networks.contains(&from.to_string()) && 
        self.supported_networks.contains(&to.to_string())
    }
    
    pub fn get_bridge_type(&self) -> String {
        "mock_bridge".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct MockCrossChainMessage {
    pub from_network: String,
    pub to_network: String,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

/// Mock consensus mechanism
pub struct MockConsensus {
    pub algorithm: String,
    pub validators: Vec<String>,
    pub current_round: u64,
}

impl MockConsensus {
    pub fn new(algorithm: &str) -> Self {
        Self {
            algorithm: algorithm.to_string(),
            validators: vec!["validator1".to_string(), "validator2".to_string()],
            current_round: 1,
        }
    }
    
    pub fn validate_block(&self, _block: &MockBlock) -> bool {
        true // Mock validation always succeeds
    }
    
    pub fn propose_block(&mut self) -> MockBlock {
        self.current_round += 1;
        MockBlock::default()
    }
}

/// Mock database for testing
pub struct MockDatabase {
    pub data: Arc<Mutex<HashMap<String, String>>>,
}

impl MockDatabase {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    pub fn get(&self, key: &str) -> Option<String> {
        let data = self.data.lock().unwrap();
        data.get(key).cloned()
    }
    
    pub fn set(&self, key: &str, value: &str) {
        let mut data = self.data.lock().unwrap();
        data.insert(key.to_string(), value.to_string());
    }
    
    pub fn delete(&self, key: &str) -> bool {
        let mut data = self.data.lock().unwrap();
        data.remove(key).is_some()
    }
    
    pub fn clear(&self) {
        let mut data = self.data.lock().unwrap();
        data.clear();
    }
}

/// Mock network layer for testing
pub struct MockNetwork {
    pub peers: Vec<String>,
    pub messages: Arc<Mutex<Vec<String>>>,
}

impl MockNetwork {
    pub fn new() -> Self {
        Self {
            peers: vec!["peer1".to_string(), "peer2".to_string()],
            messages: Arc::new(Mutex::new(vec![])),
        }
    }
    
    pub fn broadcast(&self, message: &str) {
        let mut messages = self.messages.lock().unwrap();
        messages.push(message.to_string());
    }
    
    pub fn send_to_peer(&self, _peer: &str, message: &str) {
        let mut messages = self.messages.lock().unwrap();
        messages.push(message.to_string());
    }
    
    pub fn get_peer_count(&self) -> usize {
        self.peers.len()
    }
}

/// Utility functions for creating test data
pub fn create_test_block_with_transactions(tx_count: usize) -> MockBlock {
    let mut block = MockBlock::default();
    for i in 0..tx_count {
        let mut tx = MockTransaction::default();
        tx.hash = format!("tx_hash_{}", i);
        tx.amount = (i + 1) as u64 * 10;
        block.transactions.push(tx);
    }
    block
}

pub fn create_test_climate_readings(count: usize) -> Vec<MockClimateData> {
    (0..count).map(|i| {
        MockClimateData {
            temperature: 20.0 + (i as f64 * 0.5),
            humidity: 50.0 + (i as f64 * 2.0),
            pressure: 1013.0 + (i as f64 * 0.1),
            wind_speed: 3.0 + (i as f64 * 0.2),
            location: format!("location_{}", i),
            timestamp: chrono::Utc::now().timestamp() as u64 + i as u64,
        }
    }).collect()
}