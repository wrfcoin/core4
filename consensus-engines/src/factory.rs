//! Consensus algorithm factory
//!
//! This module provides factory functions for creating different consensus
//! algorithm instances with appropriate configurations.
//!
//! Algorithm implementations are exposed through a common construction API.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::algorithms::{
    dag::DagConsensusEngine,
    dpos::DPoSConsensus,
    hotstuff::{
        HotStuffConsensusEngine, HotStuffConfig, HotStuffSigningKey, HotStuffValidator,
        ValidatorSet,
    },
    pop_adapter::PopConsensusAdapter,
    pos::ProofOfStake,
    pow::ProofOfWork,
    raft::{RaftConfig, RaftConsensusEngine},
};
use crate::equivocation::EvidenceStore;
use crate::traits::ConsensusEngine;
use crate::types::{ConsensusConfig, ConsensusError, ConsensusResult, ConsensusType};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;

/// Factory for creating consensus algorithm instances
pub struct ConsensusFactory;

impl ConsensusFactory {
    /// Create a Proof-of-Stake engine with a pre-attached durable
    /// [`EvidenceStore`] (#1218).
    ///
    /// Callers at node startup use this to guarantee that any equivocation
    /// detected during block validation is persisted through to disk and
    /// therefore survives a restart. Without this wiring, the PoS engine
    /// falls back to an in-memory evidence store and slashing proofs are
    /// lost on reboot.
    ///
    /// Prefer this over [`create`](Self::create) with
    /// `ConsensusType::ProofOfStake` whenever the running node has access
    /// to a disk-backed evidence store (the native-chain runtime always
    /// does — see `native-chain/src/consensus/evidence_store.rs`).
    pub fn create_pos_with_evidence_store(
        config: &ConsensusConfig,
        store: Arc<dyn EvidenceStore>,
    ) -> ProofOfStake {
        ProofOfStake::from_config(config).with_evidence_store(store)
    }

    /// Create a typed Proof-of-Publishing adapter directly for use with `TypedConsensusManager`.
    ///
    /// Unlike [`create`](Self::create), this returns the concrete adapter type
    /// instead of `Box<dyn ConsensusEngine>`, enabling monomorphized (static)
    /// dispatch in the production hot path.
    pub fn create_pop_adapter(config: &ConsensusConfig) -> PopConsensusAdapter {
        let pop = ProofOfPublishing::new(
            config.difficulty.unwrap_or(1) as u32,
            config.target_block_time,
            config
                .algorithm_params
                .get("min_publication_score")
                .and_then(|v| v.as_f64())
                .map(|v| (v * 1_000_000.0).round() as u64)
                .unwrap_or(600_000),
            config.max_transactions_per_block as usize,
        );
        PopConsensusAdapter::new(pop)
    }

    /// Create a consensus algorithm instance of the specified type.
    pub fn create(
        consensus_type: ConsensusType,
        config: &ConsensusConfig,
    ) -> ConsensusResult<Box<dyn ConsensusEngine>> {
        match consensus_type {
            ConsensusType::ProofOfWork => Ok(Box::new(ProofOfWork::from_config(config))),

            ConsensusType::ProofOfStake => Ok(Box::new(ProofOfStake::from_config(config))),

            ConsensusType::ProofOfPublishing => {
                Ok(Box::new(Self::create_pop_adapter(config)))
            }

            ConsensusType::DelegatedProofOfStake => {
                let active_delegates = config
                    .algorithm_params
                    .get("active_delegates")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(21) as usize;
                Ok(Box::new(DPoSConsensus::new(
                    active_delegates,
                    config.target_block_time,
                )))
            }

            ConsensusType::DAG => {
                Ok(Box::new(DagConsensusEngine::new(config.target_block_time)))
            }

            ConsensusType::Raft => {
                let node_id = config
                    .algorithm_params
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("factory-node-1")
                    .to_string();
                let raft_config = RaftConfig::default();
                // RaftConsensusManager::new is async but performs no actual I/O;
                // construct the manager synchronously via a one-shot runtime.
                let rt = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .map_err(|e| {
                        ConsensusError::InitializationError(format!(
                            "failed to create runtime for Raft init: {e}"
                        ))
                    })?;
                let engine = rt.block_on(RaftConsensusEngine::new(
                    node_id,
                    raft_config,
                    config.target_block_time,
                ))?;
                Ok(Box::new(engine))
            }

            ConsensusType::HotStuff => {
                let node_id = config
                    .algorithm_params
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("factory-node-1")
                    .to_string();
                let signing_key = HotStuffSigningKey::generate(node_id.clone());
                let self_validator = HotStuffValidator {
                    node_id: node_id.clone(),
                    public_key: signing_key.public_key_bytes().to_vec(),
                    voting_power: 1,
                    is_active: true,
                    index: 0,
                };
                let validators = ValidatorSet::new(vec![self_validator], 0);
                let hs_config = HotStuffConfig::default();
                let engine =
                    HotStuffConsensusEngine::new(hs_config, node_id, signing_key, validators)?;
                Ok(Box::new(engine))
            }

            // ProofOfUsefulness is a placeholder variant reserved for future
            // implementation.  Return a descriptive error so callers can handle
            // the absence gracefully rather than receiving a generic "unsupported"
            // message.
            ConsensusType::ProofOfUsefulness => Err(ConsensusError::ConfigurationError(
                "ProofOfUsefulness is not yet implemented; \
                 see wrfcoin/core4#1042 for the implementation roadmap"
                    .to_string(),
            )),

            _ => Err(ConsensusError::ConfigurationError(format!(
                "Unsupported consensus type: {consensus_type:?}"
            ))),
        }
    }

    /// Get a list of supported consensus types
    pub fn supported_types() -> Vec<ConsensusType> {
        vec![
            ConsensusType::ProofOfWork,
            ConsensusType::ProofOfStake,
            ConsensusType::ProofOfPublishing,
            ConsensusType::DelegatedProofOfStake,
            ConsensusType::DAG,
            ConsensusType::Raft,
            ConsensusType::HotStuff,
        ]
    }

    /// Create multiple consensus algorithms for a multi-algorithm setup
    pub fn create_multiple(
        configs: FxHashMap<ConsensusType, ConsensusConfig>,
    ) -> ConsensusResult<FxHashMap<ConsensusType, Box<dyn ConsensusEngine>>> {
        let mut algorithms = FxHashMap::default();

        for (consensus_type, config) in configs {
            let algorithm = Self::create(consensus_type, &config)?;
            algorithms.insert(consensus_type, algorithm);
        }

        Ok(algorithms)
    }

    /// Create a default configuration for a specific consensus type
    pub fn default_config(consensus_type: ConsensusType) -> ConsensusConfig {
        let mut config = ConsensusConfig::default();

        match consensus_type {
            ConsensusType::ProofOfWork => {
                config.difficulty = Some(4); // Default PoW difficulty
                config.target_block_time = 600; // 10 minutes
            }

            ConsensusType::ProofOfStake => {
                config.min_stake = Some(1000); // Default minimum stake
                config.target_block_time = 5; // 5 seconds for PoS
            }

            ConsensusType::ProofOfPublishing => {
                config.target_block_time = 15; // 15 seconds for PoP
                config.algorithm_params.insert(
                    "min_publication_score".to_string(),
                    serde_json::Value::Number(serde_json::Number::from_f64(0.6).unwrap()),
                );
                config.algorithm_params.insert(
                    "max_publications_per_block".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(10)),
                );
            }

            ConsensusType::DelegatedProofOfStake => {
                config.target_block_time = 3; // 3 seconds for DPoS
                config.algorithm_params.insert(
                    "active_delegates".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(21)),
                );
            }

            ConsensusType::DAG => {
                config.target_block_time = 1; // 1 second for DAG (parallel blocks)
            }

            ConsensusType::Raft => {
                config.target_block_time = 2; // 2 seconds for Raft
                config.algorithm_params.insert(
                    "node_id".to_string(),
                    serde_json::Value::String("raft-node-1".to_string()),
                );
            }

            ConsensusType::HotStuff => {
                config.target_block_time = 2; // 2 seconds for HotStuff
                config.algorithm_params.insert(
                    "node_id".to_string(),
                    serde_json::Value::String("hotstuff-node-1".to_string()),
                );
            }

            _ => {
                // Use defaults for other types
            }
        }

        config
    }

    /// Validate a configuration for a specific consensus type
    #[allow(clippy::excessive_nesting)]
    pub fn validate_config(
        consensus_type: ConsensusType,
        config: &ConsensusConfig,
    ) -> ConsensusResult<()> {
        // Basic validation
        if config.target_block_time == 0 {
            return Err(ConsensusError::ConfigurationError(
                "Target block time must be greater than 0".to_string(),
            ));
        }

        if config.max_block_size == 0 {
            return Err(ConsensusError::ConfigurationError(
                "Max block size must be greater than 0".to_string(),
            ));
        }

        // Type-specific validation
        match consensus_type {
            ConsensusType::ProofOfWork => {
                if config.difficulty.is_none() {
                    return Err(ConsensusError::ConfigurationError(
                        "PoW requires difficulty to be set".to_string(),
                    ));
                }

                let difficulty = config.difficulty.unwrap();
                if difficulty == 0 || difficulty > 20 {
                    return Err(ConsensusError::ConfigurationError(
                        "PoW difficulty must be between 1 and 20".to_string(),
                    ));
                }
            }

            ConsensusType::ProofOfStake => {
                if config.min_stake.is_none() {
                    return Err(ConsensusError::ConfigurationError(
                        "PoS requires min_stake to be set".to_string(),
                    ));
                }

                let min_stake = config.min_stake.unwrap();
                if min_stake == 0 {
                    return Err(ConsensusError::ConfigurationError(
                        "PoS min_stake must be greater than 0".to_string(),
                    ));
                }
            }

            ConsensusType::ProofOfPublishing => {
                if let Some(min_score) = config.algorithm_params.get("min_publication_score") {
                    if let Some(score) = min_score.as_f64() {
                        if !(0.0..=1.0).contains(&score) {
                            return Err(ConsensusError::ConfigurationError(
                                "PoP min_publication_score must be between 0.0 and 1.0".to_string(),
                            ));
                        }
                    }
                }
            }

            // DPoS, DAG, Raft, HotStuff pass basic validation above
            ConsensusType::DelegatedProofOfStake
            | ConsensusType::DAG
            | ConsensusType::Raft
            | ConsensusType::HotStuff => {}

            _ => {
                return Err(ConsensusError::ConfigurationError(format!(
                    "Validation not implemented for consensus type: {consensus_type:?}"
                )));
            }
        }

        Ok(())
    }
}

/// Convenience function to create a consensus algorithm
pub fn create_consensus(
    consensus_type: ConsensusType,
    config: &ConsensusConfig,
) -> ConsensusResult<Box<dyn ConsensusEngine>> {
    ConsensusFactory::create(consensus_type, config)
}

/// Convenience function to create a consensus algorithm with default config
pub fn create_consensus_with_defaults(
    consensus_type: ConsensusType,
) -> ConsensusResult<Box<dyn ConsensusEngine>> {
    let config = ConsensusFactory::default_config(consensus_type);
    ConsensusFactory::create(consensus_type, &config)
}

/// Builder pattern for creating configured consensus algorithms
pub struct ConsensusBuilder {
    consensus_type: ConsensusType,
    config: ConsensusConfig,
}

impl ConsensusBuilder {
    /// Create a new builder for the specified consensus type
    pub fn new(consensus_type: ConsensusType) -> Self {
        let config = ConsensusFactory::default_config(consensus_type);
        Self {
            consensus_type,
            config,
        }
    }

    /// Set the target block time
    pub fn target_block_time(mut self, seconds: u64) -> Self {
        self.config.target_block_time = seconds;
        self
    }

    /// Set the difficulty (for PoW)
    pub fn difficulty(mut self, difficulty: u64) -> Self {
        self.config.difficulty = Some(difficulty);
        self
    }

    /// Set the minimum stake (for PoS)
    pub fn min_stake(mut self, stake: u64) -> Self {
        self.config.min_stake = Some(stake);
        self
    }

    /// Set the maximum block size
    pub fn max_block_size(mut self, size: u64) -> Self {
        self.config.max_block_size = size;
        self
    }

    /// Set algorithm-specific parameter
    pub fn algorithm_param(mut self, key: String, value: serde_json::Value) -> Self {
        self.config.algorithm_params.insert(key, value);
        self
    }

    /// Build the consensus algorithm
    pub fn build(self) -> ConsensusResult<Box<dyn ConsensusEngine>> {
        ConsensusFactory::validate_config(self.consensus_type, &self.config)?;
        ConsensusFactory::create(self.consensus_type, &self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_pow() {
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfWork);
        assert_eq!(config.difficulty, Some(4));
        assert_eq!(config.target_block_time, 600);
    }

    #[test]
    fn test_default_config_pos() {
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfStake);
        assert_eq!(config.min_stake, Some(1000));
        assert_eq!(config.target_block_time, 5);
    }

    #[test]
    fn test_default_config_pop() {
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        assert_eq!(config.target_block_time, 15);
        assert!(config.algorithm_params.contains_key("min_publication_score"));
    }

    #[test]
    fn test_config_validation_pow_valid() {
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfWork);
        let result = ConsensusFactory::validate_config(ConsensusType::ProofOfWork, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_validation_pow_zero_difficulty() {
        let mut config = ConsensusFactory::default_config(ConsensusType::ProofOfWork);
        config.difficulty = Some(0);
        let result = ConsensusFactory::validate_config(ConsensusType::ProofOfWork, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validation_pos_valid() {
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfStake);
        let result = ConsensusFactory::validate_config(ConsensusType::ProofOfStake, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_validation_pos_zero_stake() {
        let mut config = ConsensusFactory::default_config(ConsensusType::ProofOfStake);
        config.min_stake = Some(0);
        let result = ConsensusFactory::validate_config(ConsensusType::ProofOfStake, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validation_zero_block_time() {
        let mut config = ConsensusFactory::default_config(ConsensusType::ProofOfWork);
        config.target_block_time = 0;
        let result = ConsensusFactory::validate_config(ConsensusType::ProofOfWork, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_supported_types() {
        let types = ConsensusFactory::supported_types();
        assert_eq!(types.len(), 7);
        assert!(types.contains(&ConsensusType::ProofOfWork));
        assert!(types.contains(&ConsensusType::ProofOfStake));
        assert!(types.contains(&ConsensusType::ProofOfPublishing));
        assert!(types.contains(&ConsensusType::DelegatedProofOfStake));
        assert!(types.contains(&ConsensusType::DAG));
        assert!(types.contains(&ConsensusType::Raft));
        assert!(types.contains(&ConsensusType::HotStuff));
    }

    #[test]
    fn test_unsupported_type() {
        let config = ConsensusConfig::default();
        let result = ConsensusFactory::create(ConsensusType::ProofOfPresence, &config);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // ProofOfUsefulness error-path tests (issue #1042)
    // -----------------------------------------------------------------------

    /// `ConsensusFactory::create` must return `Err` for `ProofOfUsefulness`,
    /// not panic or succeed.
    #[test]
    fn proof_of_usefulness_create_returns_err() {
        let config = ConsensusConfig::default();
        let result = ConsensusFactory::create(ConsensusType::ProofOfUsefulness, &config);
        assert!(
            result.is_err(),
            "ProofOfUsefulness must return Err until implemented"
        );
    }

    /// The error message must mention "ProofOfUsefulness" and "not yet implemented"
    /// so operators can distinguish it from other unsupported-type errors.
    #[test]
    fn proof_of_usefulness_error_is_descriptive() {
        let config = ConsensusConfig::default();
        let err = ConsensusFactory::create(ConsensusType::ProofOfUsefulness, &config)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("ProofOfUsefulness"),
            "error message must contain 'ProofOfUsefulness', got: {msg}"
        );
        assert!(
            msg.contains("not yet implemented"),
            "error message must contain 'not yet implemented', got: {msg}"
        );
    }

    /// `validate_config` must also return an error for `ProofOfUsefulness`
    /// (falls through to the catch-all `_ =>` branch).
    #[test]
    fn proof_of_usefulness_validate_config_returns_err() {
        let config = ConsensusConfig::default();
        let result =
            ConsensusFactory::validate_config(ConsensusType::ProofOfUsefulness, &config);
        assert!(
            result.is_err(),
            "validate_config must return Err for ProofOfUsefulness"
        );
    }

    /// `create_multiple` propagates the `Err` from PoU instead of silently
    /// dropping it — callers must handle or avoid including PoU in a multi-map.
    #[test]
    fn create_multiple_propagates_pou_error() {
        let mut configs = rustc_hash::FxHashMap::default();
        configs.insert(
            ConsensusType::ProofOfUsefulness,
            ConsensusConfig::default(),
        );
        let result = ConsensusFactory::create_multiple(configs);
        assert!(
            result.is_err(),
            "create_multiple must propagate Err when map contains ProofOfUsefulness"
        );
    }

    /// `ConsensusBuilder` built for `ProofOfUsefulness` must return `Err`
    /// (it validates then calls create).
    #[test]
    fn builder_proof_of_usefulness_returns_err() {
        // validate_config returns Err for PoU, so build() must fail before create()
        let result = ConsensusBuilder::new(ConsensusType::ProofOfUsefulness).build();
        assert!(
            result.is_err(),
            "ConsensusBuilder::build must return Err for ProofOfUsefulness"
        );
    }

    /// The `supported_types()` list must NOT include `ProofOfUsefulness`
    /// (it is not yet production-ready).
    #[test]
    fn proof_of_usefulness_not_in_supported_types() {
        let supported = ConsensusFactory::supported_types();
        assert!(
            !supported.contains(&ConsensusType::ProofOfUsefulness),
            "ProofOfUsefulness must not appear in supported_types() until implemented"
        );
    }

    #[test]
    fn test_create_pop_succeeds() {
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        let result = ConsensusFactory::create(ConsensusType::ProofOfPublishing, &config);
        assert!(result.is_ok());
        let engine = result.unwrap();
        assert_eq!(engine.algorithm_name(), "ProofOfPublishing");
    }

    #[test]
    fn pow_factory_creates_engine() {
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfWork);
        let engine = ConsensusFactory::create(ConsensusType::ProofOfWork, &config).unwrap();
        assert_eq!(engine.algorithm_name(), "ProofOfWork");
    }

    #[test]
    fn pos_factory_creates_engine() {
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfStake);
        let engine = ConsensusFactory::create(ConsensusType::ProofOfStake, &config).unwrap();
        assert_eq!(engine.algorithm_name(), "ProofOfStake");
    }

    /// #1218: the factory helper must attach the passed-in evidence
    /// store to the constructed PoS engine, not fall back to the default
    /// in-memory store. We verify this by persisting evidence through
    /// the injected handle and observing it via the engine's accessor.
    #[tokio::test]
    async fn pos_factory_with_evidence_store_attaches_durable_backend() {
        use crate::equivocation::{
            EquivocationEvidence, EvidenceStore, InMemoryEvidenceStore,
        };
        use wrfcoin_shared_protocol::crypto::hash::Hash;

        let shared: Arc<dyn EvidenceStore> = Arc::new(InMemoryEvidenceStore::new());
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfStake);
        let engine =
            ConsensusFactory::create_pos_with_evidence_store(&config, Arc::clone(&shared));

        // Persist via the shared handle — the engine must see it.
        shared
            .persist(EquivocationEvidence {
                validator: "v".to_string(),
                height: 1,
                first_block_hash: Hash::from([1u8; 32]),
                second_block_hash: Hash::from([2u8; 32]),
                detected_at: 0,
            })
            .await
            .unwrap();

        let via_engine = engine.evidence_store();
        assert_eq!(
            via_engine.count().await.unwrap(),
            1,
            "engine must use the injected store, not the default"
        );
    }

    #[test]
    fn dpos_factory_creates_engine() {
        let config = ConsensusFactory::default_config(ConsensusType::DelegatedProofOfStake);
        let engine =
            ConsensusFactory::create(ConsensusType::DelegatedProofOfStake, &config).unwrap();
        assert_eq!(engine.algorithm_name(), "DelegatedProofOfStake");
    }

    #[test]
    fn dag_factory_creates_engine() {
        let config = ConsensusFactory::default_config(ConsensusType::DAG);
        let engine = ConsensusFactory::create(ConsensusType::DAG, &config).unwrap();
        assert_eq!(engine.algorithm_name(), "DAG");
    }

    #[test]
    fn raft_factory_creates_engine() {
        let config = ConsensusFactory::default_config(ConsensusType::Raft);
        let engine = ConsensusFactory::create(ConsensusType::Raft, &config).unwrap();
        assert_eq!(engine.algorithm_name(), "Raft");
    }

    #[test]
    fn hotstuff_factory_creates_engine() {
        let config = ConsensusFactory::default_config(ConsensusType::HotStuff);
        let engine = ConsensusFactory::create(ConsensusType::HotStuff, &config).unwrap();
        assert_eq!(engine.algorithm_name(), "HotStuff");
    }
}
