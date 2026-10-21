//! Core consensus engine manager for coordinating multiple consensus algorithms
//!
//! This module provides the main consensus engine manager that can dynamically
//! switch between different consensus algorithms and manage their lifecycle.

use rustc_hash::FxHashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::algorithms::pop_adapter::PopConsensusAdapter;
use crate::traits::{ConsensusEngine, NetworkStats};
use crate::types::{ConsensusConfig, ConsensusError, ConsensusResult, ConsensusType};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::mempool::pool::Mempool;

/// Core consensus engine manager that coordinates multiple consensus algorithms
///
/// Named `ConsensusEngineManager` to avoid collision with the `ConsensusEngine` trait.
/// This struct manages a registry of algorithms and supports runtime hot-swapping.
pub struct ConsensusEngineManager {
    /// Currently active consensus algorithm
    active_algorithm: Arc<RwLock<Box<dyn ConsensusEngine>>>,

    /// Registry of available consensus algorithms
    algorithm_registry: FxHashMap<ConsensusType, Box<dyn ConsensusEngine>>,

    /// Current consensus configuration
    config: ConsensusConfig,

    /// Network statistics for dynamic adjustment
    network_stats: Arc<RwLock<NetworkStats>>,

    /// Algorithm switching enabled
    switching_enabled: bool,
}

impl std::fmt::Debug for ConsensusEngineManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsensusEngineManager")
            .field("config", &self.config)
            .field("switching_enabled", &self.switching_enabled)
            .field("algorithm_registry_count", &self.algorithm_registry.len())
            .field("active_algorithm", &"<dyn ConsensusEngine>")
            .field("network_stats", &"Arc<RwLock<NetworkStats>>")
            .finish()
    }
}

impl ConsensusEngineManager {
    /// Create a new consensus engine manager with the specified initial algorithm.
    ///
    /// This is a synchronous constructor — no I/O is performed.
    pub fn new(
        initial_algorithm: Box<dyn ConsensusEngine>,
        config: ConsensusConfig,
    ) -> Self {
        let network_stats = NetworkStats {
            avg_block_time: config.target_block_time as f64,
            tx_throughput: 0.0,
            hash_rate: None,
            total_stake: None,
            active_validators: 1,
            network_latency_ms: 100.0,
        };

        Self {
            active_algorithm: Arc::new(RwLock::new(initial_algorithm)),
            algorithm_registry: FxHashMap::default(),
            config,
            network_stats: Arc::new(RwLock::new(network_stats)),
            switching_enabled: false,
        }
    }

    /// Register a consensus algorithm with the engine manager
    pub fn register_algorithm(
        &mut self,
        consensus_type: ConsensusType,
        algorithm: Box<dyn ConsensusEngine>,
    ) {
        self.algorithm_registry.insert(consensus_type, algorithm);
    }

    /// Switch to a different consensus algorithm
    pub async fn switch_algorithm(&mut self, consensus_type: ConsensusType) -> ConsensusResult<()> {
        if !self.switching_enabled {
            return Err(ConsensusError::ConfigurationError(
                "Algorithm switching is disabled".to_string(),
            ));
        }

        let algorithm = self
            .algorithm_registry
            .remove(&consensus_type)
            .ok_or_else(|| {
                ConsensusError::ConfigurationError(format!(
                    "Algorithm {consensus_type:?} not registered"
                ))
            })?;

        // Configure the new algorithm
        let mut configured_algorithm = algorithm;
        configured_algorithm.configure(&self.config).await?;

        // Replace the active algorithm
        let mut active = self.active_algorithm.write().await;
        *active = configured_algorithm;

        Ok(())
    }

    /// Enable or disable algorithm switching
    pub fn set_switching_enabled(&mut self, enabled: bool) {
        self.switching_enabled = enabled;
    }

    /// Validate a block using the current consensus algorithm
    pub async fn validate_block(
        &self,
        block: &Block,
        chain: &Blockchain,
    ) -> ConsensusResult<bool> {
        let algorithm = self.active_algorithm.read().await;
        algorithm.validate_block(block, chain).await
    }

    /// Create a block using the current consensus algorithm
    pub async fn create_block(
        &self,
        transactions: Vec<Transaction>,
        previous_hash: &str,
        miner_address: &str,
        chain: &Blockchain,
    ) -> ConsensusResult<Block> {
        let algorithm = self.active_algorithm.read().await;
        algorithm
            .create_block(transactions, previous_hash, miner_address, chain)
            .await
    }

    /// Get the current difficulty
    pub async fn get_difficulty(&self) -> u64 {
        let algorithm = self.active_algorithm.read().await;
        algorithm.get_difficulty().await
    }

    /// Get the name of the current algorithm
    pub async fn get_algorithm_name(&self) -> String {
        let algorithm = self.active_algorithm.read().await;
        algorithm.algorithm_name().to_string()
    }

    /// Update the consensus configuration
    pub async fn update_config(&mut self, config: ConsensusConfig) -> ConsensusResult<()> {
        self.config = config.clone();

        // Update the active algorithm configuration
        let mut algorithm = self.active_algorithm.write().await;
        algorithm.configure(&config).await?;

        Ok(())
    }

    /// Get current configuration
    pub fn get_config(&self) -> &ConsensusConfig {
        &self.config
    }

    /// Update network statistics
    pub async fn update_network_stats(&self, stats: NetworkStats) {
        let mut current_stats = self.network_stats.write().await;
        *current_stats = stats;
    }

    /// Get current network statistics
    pub async fn get_network_stats(&self) -> NetworkStats {
        self.network_stats.read().await.clone()
    }

    /// Get metrics from the current algorithm
    pub async fn get_metrics(&self) -> FxHashMap<String, serde_json::Value> {
        let algorithm = self.active_algorithm.read().await;
        let mut metrics = algorithm.get_metrics();

        // Add engine-level metrics
        metrics.insert(
            "switching_enabled".to_string(),
            self.switching_enabled.into(),
        );
        metrics.insert(
            "registered_algorithms".to_string(),
            self.algorithm_registry.len().into(),
        );

        metrics
    }

    /// Get a list of registered algorithm types
    pub fn get_registered_algorithms(&self) -> Vec<ConsensusType> {
        self.algorithm_registry.keys().cloned().collect()
    }

    /// Check if an algorithm is registered
    pub fn is_algorithm_registered(&self, consensus_type: &ConsensusType) -> bool {
        self.algorithm_registry.contains_key(consensus_type)
    }
}

// ---------------------------------------------------------------------------
// TypedConsensusManager — static dispatch, single-algorithm production path
// ---------------------------------------------------------------------------

/// Single-algorithm consensus manager using static (monomorphized) dispatch.
///
/// Compared to [`ConsensusEngineManager`], this eliminates:
/// - The outer `Arc<RwLock<Box<dyn ConsensusEngine>>>` — one fewer async lock
///   acquisition on every `validate_block` / `create_block` call.
/// - The vtable call (`dyn ConsensusEngine`) — the compiler can see through to
///   the concrete algorithm and inline or specialise accordingly.
///
/// Use this when only one consensus algorithm runs in production (the default
/// PoP path). For multi-algorithm / hot-swap scenarios keep
/// [`ConsensusEngineManager`].
pub struct TypedConsensusManager<E: ConsensusEngine> {
    engine: E,
    config: ConsensusConfig,
}

impl<E: ConsensusEngine + std::fmt::Debug> std::fmt::Debug for TypedConsensusManager<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedConsensusManager")
            .field("engine", &self.engine)
            .field("config", &self.config)
            .finish()
    }
}

impl<E: ConsensusEngine> TypedConsensusManager<E> {
    /// Create a new typed consensus manager wrapping a concrete engine.
    pub fn new(engine: E, config: ConsensusConfig) -> Self {
        Self { engine, config }
    }

    /// Validate a block — direct call, no outer lock or vtable dispatch.
    #[inline]
    pub async fn validate_block(
        &self,
        block: &Block,
        chain: &Blockchain,
    ) -> ConsensusResult<bool> {
        self.engine.validate_block(block, chain).await
    }

    /// Create a block — direct call, no outer lock or vtable dispatch.
    #[inline]
    pub async fn create_block(
        &self,
        transactions: Vec<Transaction>,
        previous_hash: &str,
        miner_address: &str,
        chain: &Blockchain,
    ) -> ConsensusResult<Block> {
        self.engine
            .create_block(transactions, previous_hash, miner_address, chain)
            .await
    }

    /// Return the current difficulty from the underlying algorithm.
    pub async fn get_difficulty(&self) -> u64 {
        self.engine.get_difficulty().await
    }

    /// Return the algorithm name.
    pub fn algorithm_name(&self) -> &'static str {
        self.engine.algorithm_name()
    }

    /// Return a reference to the current configuration.
    pub fn get_config(&self) -> &ConsensusConfig {
        &self.config
    }

    /// Return algorithm-specific metrics.
    pub fn get_metrics(&self) -> FxHashMap<String, serde_json::Value> {
        self.engine.get_metrics()
    }
}

impl TypedConsensusManager<PopConsensusAdapter> {
    /// Produce a PoP block using the stateful path that mutates chain state.
    #[inline]
    pub async fn create_pop_block_with_state(
        &self,
        blockchain: Arc<RwLock<Blockchain>>,
        mempool: Arc<RwLock<Mempool>>,
        miner_address: &str,
    ) -> ConsensusResult<Block> {
        self.engine
            .create_next_block(blockchain, mempool, miner_address)
            .await
    }
}

// ---------------------------------------------------------------------------

/// Builder for creating consensus engine managers with multiple algorithms
pub struct ConsensusEngineBuilder {
    algorithms: FxHashMap<ConsensusType, Box<dyn ConsensusEngine>>,
    config: ConsensusConfig,
    switching_enabled: bool,
}

impl ConsensusEngineBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            algorithms: FxHashMap::default(),
            config: ConsensusConfig::default(),
            switching_enabled: false,
        }
    }

    /// Add an algorithm to the builder
    pub fn with_algorithm(
        mut self,
        consensus_type: ConsensusType,
        algorithm: Box<dyn ConsensusEngine>,
    ) -> Self {
        self.algorithms.insert(consensus_type, algorithm);
        self
    }

    /// Set the configuration
    pub fn with_config(mut self, config: ConsensusConfig) -> Self {
        self.config = config;
        self
    }

    /// Enable algorithm switching
    pub fn with_switching_enabled(mut self, enabled: bool) -> Self {
        self.switching_enabled = enabled;
        self
    }

    /// Build the consensus engine manager with the specified initial algorithm
    pub async fn build(
        mut self,
        initial_algorithm_type: ConsensusType,
    ) -> ConsensusResult<ConsensusEngineManager> {
        let initial_algorithm =
            self.algorithms
                .remove(&initial_algorithm_type)
                .ok_or_else(|| {
                    ConsensusError::ConfigurationError(format!(
                        "Initial algorithm {initial_algorithm_type:?} not provided"
                    ))
                })?;

        let mut engine = ConsensusEngineManager::new(initial_algorithm, self.config);

        // Register remaining algorithms
        for (consensus_type, algorithm) in self.algorithms {
            engine.register_algorithm(consensus_type, algorithm);
        }

        engine.set_switching_enabled(self.switching_enabled);

        Ok(engine)
    }
}

impl Default for ConsensusEngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::dpos::{DPoSConsensus, DPoSState};
    use crate::algorithms::pow::ProofOfWork;
    use crate::types::{ConsensusConfig, ConsensusType};

    /// Build a DPoS engine with one registered delegate, wrapped as trait object.
    fn make_dpos() -> Box<dyn ConsensusEngine> {
        let mut state = DPoSState::with_config(100, 500);
        state
            .register_validator("delegate-1".to_string(), 1000, 500) // 5% in bps
            .unwrap();
        Box::new(DPoSConsensus::with_state(state, 3, 6))
    }

    /// Happy path: manager starts with the provided algorithm.
    #[tokio::test]
    async fn test_manager_initial_algorithm_name() {
        let manager = ConsensusEngineManager::new(make_dpos(), ConsensusConfig::default());
        assert_eq!(manager.get_algorithm_name().await, "DelegatedProofOfStake");
    }

    /// Edge case: switching is disabled by default and returns an error.
    #[tokio::test]
    async fn test_manager_switching_disabled_by_default() {
        let mut manager = ConsensusEngineManager::new(make_dpos(), ConsensusConfig::default());
        let result = manager.switch_algorithm(ConsensusType::ProofOfWork).await;
        assert!(
            result.is_err(),
            "switch must fail when switching_enabled is false"
        );
        assert!(
            matches!(result.unwrap_err(), ConsensusError::ConfigurationError(_)),
            "expected ConfigurationError"
        );
    }

    /// Happy path: registered algorithm appears in the registry.
    #[test]
    fn test_manager_register_and_query() {
        let mut manager = ConsensusEngineManager::new(make_dpos(), ConsensusConfig::default());
        manager.register_algorithm(
            ConsensusType::ProofOfWork,
            Box::new(ProofOfWork::new(1, 10)),
        );

        assert!(manager.is_algorithm_registered(&ConsensusType::ProofOfWork));
        assert!(!manager.is_algorithm_registered(&ConsensusType::ProofOfStake));

        let registered = manager.get_registered_algorithms();
        assert_eq!(registered.len(), 1);
    }

    /// Happy path: hot-swap to a registered algorithm succeeds.
    #[tokio::test]
    async fn test_manager_switch_algorithm_success() {
        let mut manager = ConsensusEngineManager::new(make_dpos(), ConsensusConfig::default());
        manager.register_algorithm(
            ConsensusType::ProofOfWork,
            Box::new(ProofOfWork::new(1, 10)),
        );
        manager.set_switching_enabled(true);

        manager
            .switch_algorithm(ConsensusType::ProofOfWork)
            .await
            .unwrap();

        assert_eq!(manager.get_algorithm_name().await, "ProofOfWork");
        // After the switch, PoW is no longer in the registry (it was moved out).
        assert!(!manager.is_algorithm_registered(&ConsensusType::ProofOfWork));
    }

    /// Error path: switch to an unregistered algorithm fails.
    #[tokio::test]
    async fn test_manager_switch_to_unregistered_fails() {
        let mut manager = ConsensusEngineManager::new(make_dpos(), ConsensusConfig::default());
        manager.set_switching_enabled(true);

        let result = manager.switch_algorithm(ConsensusType::ProofOfStake).await;
        assert!(
            result.is_err(),
            "switching to an unregistered algorithm must fail"
        );
    }

    /// Metrics include both algorithm-specific and engine-level keys.
    #[tokio::test]
    async fn test_manager_metrics_keys() {
        let manager = ConsensusEngineManager::new(make_dpos(), ConsensusConfig::default());
        let metrics = manager.get_metrics().await;

        assert!(
            metrics.contains_key("algorithm"),
            "metrics must include 'algorithm'"
        );
        assert!(
            metrics.contains_key("switching_enabled"),
            "metrics must include 'switching_enabled'"
        );
        assert!(
            metrics.contains_key("registered_algorithms"),
            "metrics must include 'registered_algorithms'"
        );
    }

    /// TypedConsensusManager wraps a concrete engine with direct dispatch.
    #[tokio::test]
    async fn test_typed_manager_algorithm_name_and_difficulty() {
        let state = {
            let mut s = DPoSState::with_config(100, 500);
            s.register_validator("del".to_string(), 1000, 0).unwrap(); // 0% in bps
            s
        };
        let engine = DPoSConsensus::with_state(state, 1, 6);
        let typed = TypedConsensusManager::new(engine, ConsensusConfig::default());

        assert_eq!(typed.algorithm_name(), "DelegatedProofOfStake");
        assert_eq!(typed.get_difficulty().await, 1);
    }
}
