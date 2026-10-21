//! Consensus algorithm traits and interfaces
//!
//! This module defines the core traits that all consensus algorithms must implement
//! to provide a unified interface for the consensus engine.

use crate::types::{ConsensusConfig, ConsensusResult};
use async_trait::async_trait;
use rustc_hash::FxHashMap;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;

/// Core trait that all consensus algorithms must implement
///
/// This trait provides a unified interface for different consensus mechanisms
/// including block validation, block creation, and difficulty management.
///
/// The `mine_next_block` method from the original blockchain-core trait has been
/// intentionally removed: it accepted `Arc<RwLock<>>` runtime types that belong
/// in the node runtime, not in a reusable consensus library.
#[async_trait]
pub trait ConsensusEngine: Send + Sync + std::fmt::Debug {
    /// Validate a block according to this consensus algorithm's rules
    ///
    /// # Arguments
    /// * `block` - The block to validate
    /// * `chain` - Current blockchain state for validation context
    ///
    /// # Returns
    /// * `Ok(true)` if the block is valid
    /// * `Ok(false)` if the block is invalid but not an error
    /// * `Err(ConsensusError)` if validation fails due to an error
    async fn validate_block(
        &self,
        block: &Block,
        chain: &Blockchain,
    ) -> ConsensusResult<bool>;

    /// Create a new block using this consensus algorithm
    ///
    /// # Arguments
    /// * `transactions` - Transactions to include in the block
    /// * `previous_hash` - Hash of the previous block
    /// * `miner_address` - Address of the block creator/validator
    /// * `chain` - Current blockchain state
    ///
    /// # Returns
    /// * `Ok(Block)` if block creation succeeds
    /// * `Err(ConsensusError)` if block creation fails
    async fn create_block(
        &self,
        transactions: Vec<Transaction>,
        previous_hash: &str,
        miner_address: &str,
        chain: &Blockchain,
    ) -> ConsensusResult<Block>;

    /// Get the current difficulty for this consensus algorithm
    ///
    /// # Returns
    /// The current difficulty level as a u64
    async fn get_difficulty(&self) -> u64;

    /// Get the name of this consensus algorithm
    ///
    /// # Returns
    /// A static string identifying the algorithm
    fn algorithm_name(&self) -> &'static str;

    /// Configure the consensus algorithm with new parameters
    ///
    /// # Arguments
    /// * `config` - New configuration parameters
    ///
    /// # Returns
    /// * `Ok(())` if configuration succeeds
    /// * `Err(ConsensusError)` if configuration fails
    async fn configure(&mut self, config: &ConsensusConfig) -> ConsensusResult<()>;

    /// Get algorithm-specific metrics and statistics
    ///
    /// # Returns
    /// A map of metric names to their values
    fn get_metrics(&self) -> FxHashMap<String, serde_json::Value>;
}

/// Trait for consensus algorithms that support dynamic parameter adjustment
#[async_trait]
pub trait DynamicConsensus: ConsensusEngine {
    /// Adjust algorithm parameters based on network conditions
    ///
    /// # Arguments
    /// * `network_stats` - Current network statistics
    ///
    /// # Returns
    /// * `Ok(())` if adjustment succeeds
    /// * `Err(ConsensusError)` if adjustment fails
    async fn adjust_parameters(&mut self, network_stats: &NetworkStats) -> ConsensusResult<()>;
}

/// Network statistics for dynamic consensus adjustment
#[derive(Debug, Clone)]
pub struct NetworkStats {
    /// Average block time in seconds
    pub avg_block_time: f64,

    /// Current transaction throughput (tx/sec)
    pub tx_throughput: f64,

    /// Network hash rate (for PoW algorithms)
    pub hash_rate: Option<u64>,

    /// Total stake (for PoS algorithms)
    pub total_stake: Option<u64>,

    /// Number of active validators/miners
    pub active_validators: u64,

    /// Network latency metrics
    pub network_latency_ms: f64,
}
