//! Probabilistic Finality Engine with BFT Checkpoints
//!
//! Combines probabilistic finality (where confidence grows with block depth)
//! with deterministic BFT checkpoints for absolute finality guarantees. The
//! engine tracks chain reorganizations, calculates economic finality based on
//! slashing costs, and provides a unified finality oracle for querying the
//! probability that any given block is final.
//!
//! Extracted from `blockchain-core/src/consensus/probabilistic_finality.rs`.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use rustc_hash::FxHashMap;

/// Default number of blocks between periodic checkpoints.
pub const DEFAULT_CHECKPOINT_INTERVAL: u64 = 100;

/// Default adversary threshold assumption (fraction of total stake/hashrate).
pub const DEFAULT_ADVERSARY_THRESHOLD: f64 = 0.33;

/// Default number of confirmations for near-certain probabilistic finality.
pub const DEFAULT_DEEP_CONFIRMATION_DEPTH: u64 = 30;

/// Errors that can occur in the probabilistic finality engine.
#[derive(Debug, Clone, thiserror::Error)]
pub enum FinalityError {
    #[error("Block not found: {0}")]
    BlockNotFound(String),

    #[error("Checkpoint already exists at height {0}")]
    CheckpointAlreadyExists(u64),

    #[error("Insufficient validator signatures: got {got}, need {need}")]
    InsufficientSignatures { got: usize, need: usize },

    #[error("Invalid checkpoint: {0}")]
    InvalidCheckpoint(String),

    #[error("Reorganization detected at height {height}: expected {expected}, got {actual}")]
    ReorgDetected {
        height: u64,
        expected: String,
        actual: String,
    },

    #[error("Cannot revert past finalized checkpoint at height {0}")]
    RevertPastCheckpoint(u64),
}

/// The type of a finality checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckpointType {
    /// Created at regular intervals (every N blocks).
    Periodic,
    /// Triggered by specific events (e.g., high-value transactions).
    Triggered,
    /// Created in response to a detected anomaly or attack.
    Emergency,
}

/// A BFT finality checkpoint that provides deterministic finality for a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalityCheckpoint {
    /// Hash of the block being checkpointed.
    pub block_hash: String,
    /// Height of the checkpointed block.
    pub block_height: u64,
    /// Validator signatures attesting to this checkpoint.
    pub validator_signatures: Vec<ValidatorSignature>,
    /// Type of checkpoint.
    pub checkpoint_type: CheckpointType,
    /// Timestamp when the checkpoint was created.
    pub timestamp: u64,
    /// Hash of the checkpoint data for integrity verification.
    pub checkpoint_hash: String,
}

impl FinalityCheckpoint {
    /// Compute the checkpoint hash from its contents.
    fn compute_hash(
        block_hash: &str,
        block_height: u64,
        checkpoint_type: &CheckpointType,
        timestamp: u64,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"wrfcoin-checkpoint-v1|");
        hasher.update(block_hash.as_bytes());
        hasher.update(block_height.to_be_bytes());
        hasher.update(format!("{checkpoint_type:?}").as_bytes());
        hasher.update(timestamp.to_be_bytes());
        hex::encode(hasher.finalize())
    }
}

/// A validator's signature attesting to a checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    /// Address of the signing validator.
    pub validator_address: String,
    /// Signature bytes.
    pub signature: Vec<u8>,
    /// Validator's stake weight at time of signing.
    pub stake_weight: u64,
}

/// Snapshot of known block metadata used for finality calculations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMetadata {
    /// Block hash.
    pub hash: String,
    /// Block height.
    pub height: u64,
    /// Timestamp of the block.
    pub timestamp: u64,
    /// Total stake/hashrate weight behind this block.
    pub cumulative_weight: u64,
}

/// Parameters for calculating confirmation probability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationParams {
    /// Fraction of total weight assumed to be adversarial (0.0 to 0.5).
    pub adversary_threshold: f64,
    /// Total network weight (hash rate or total stake).
    pub total_network_weight: u64,
    /// Base confirmation reliability per depth level.
    pub base_confidence_per_depth: f64,
}

impl Default for ConfirmationParams {
    fn default() -> Self {
        Self {
            adversary_threshold: DEFAULT_ADVERSARY_THRESHOLD,
            total_network_weight: 1_000_000,
            base_confidence_per_depth: 0.15,
        }
    }
}

/// Calculates the probability that a block is final based on depth and network parameters.
#[derive(Debug, Clone)]
pub struct ConfirmationProbability {
    params: ConfirmationParams,
}

impl ConfirmationProbability {
    /// Create a new confirmation probability calculator.
    pub fn new(params: ConfirmationParams) -> Self {
        Self { params }
    }

    /// Calculate the probability of finality for a block at a given depth.
    ///
    /// Uses an exponential model: P(final) = 1 - q^depth, where q is the
    /// adversary's probability of catching up (related to adversary threshold).
    /// With an adversary controlling fraction `a` of the network, the probability
    /// of a successful reorg decreases exponentially with depth.
    ///
    /// Returns a value in the range [0.0, 1.0].
    pub fn calculate(&self, depth: u64, adversary_fraction: Option<f64>) -> f64 {
        if depth == 0 {
            return 0.0;
        }

        let a = adversary_fraction.unwrap_or(self.params.adversary_threshold);

        // Clamp adversary fraction to valid range
        let a = a.clamp(0.0, 0.5);

        // For PoW: probability of attacker catching up from `depth` blocks behind
        // is approximately (a / (1 - a))^depth for a < 0.5.
        // For PoS: similar model with stake-weighted probabilities.
        let q = if a >= 0.5 {
            // Adversary has majority -- finality is never guaranteed
            return 0.0;
        } else {
            a / (1.0 - a)
        };

        let reorg_probability = q.powi(depth as i32);
        (1.0 - reorg_probability).clamp(0.0, 1.0)
    }

    /// Calculate finality probability incorporating cumulative weight.
    ///
    /// Blocks with higher cumulative weight relative to the network total
    /// receive a confidence boost.
    pub fn calculate_with_weight(&self, depth: u64, cumulative_weight: u64) -> f64 {
        let base_probability = self.calculate(depth, None);

        // Weight factor: blocks backed by more weight are harder to revert
        let weight_ratio = if self.params.total_network_weight > 0 {
            (cumulative_weight as f64 / self.params.total_network_weight as f64).min(1.0)
        } else {
            0.0
        };

        // Combine depth-based and weight-based confidence
        let combined = base_probability * 0.7 + weight_ratio * 0.3;
        combined.clamp(0.0, 1.0)
    }
}

/// Calculates economic finality based on the cost to revert a block.
///
/// Economic finality is determined by the total amount at stake that would be
/// slashed if a reorg were attempted. When the cost to revert exceeds the
/// potential gain, the block is considered economically final.
#[derive(Debug, Clone)]
pub struct EconomicFinality {
    /// Total staked amount across all validators.
    pub total_staked: u64,
    /// Fraction of stake that would be slashed for equivocation.
    pub slashing_fraction: f64,
    /// Number of validators that would need to collude for a reorg.
    pub min_colluding_validators: usize,
}

impl EconomicFinality {
    /// Create a new economic finality calculator.
    pub fn new(total_staked: u64, slashing_fraction: f64, min_colluding_validators: usize) -> Self {
        Self {
            total_staked,
            slashing_fraction,
            min_colluding_validators,
        }
    }

    /// Calculate the cost (in tokens) to revert a block.
    ///
    /// The cost is the total amount that would be slashed if the minimum number
    /// of validators required for a reorg all equivocated.
    pub fn cost_to_revert(&self) -> u64 {
        let per_validator_stake = if self.min_colluding_validators > 0 {
            self.total_staked / self.min_colluding_validators as u64
        } else {
            0
        };

        let slashable = (per_validator_stake as f64 * self.slashing_fraction) as u64;
        slashable * self.min_colluding_validators as u64
    }

    /// Calculate economic finality as a probability (0.0 to 1.0).
    ///
    /// Returns 1.0 when the cost to revert exceeds a threshold relative to
    /// the total staked amount, indicating economic finality.
    pub fn economic_finality_score(&self, block_value_at_risk: u64) -> f64 {
        let cost = self.cost_to_revert();
        if block_value_at_risk == 0 {
            return 1.0;
        }

        // Economic finality = cost_to_revert / value_at_risk, clamped to [0, 1]
        let ratio = cost as f64 / block_value_at_risk as f64;
        ratio.clamp(0.0, 1.0)
    }
}

/// Monitors for chain reorganizations and adjusts finality estimates.
#[derive(Debug, Clone)]
pub struct ReorgDetector {
    /// Known block hashes by height.
    known_blocks: FxHashMap<u64, String>,
    /// Recorded reorganizations.
    reorgs: Vec<ReorgEvent>,
    /// Maximum reorg depth observed.
    max_reorg_depth: u64,
}

/// A recorded chain reorganization event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorgEvent {
    /// Height at which the reorg forked from the canonical chain.
    pub fork_height: u64,
    /// Depth of the reorganization (number of blocks reverted).
    pub depth: u64,
    /// Previous block hash at the fork point.
    pub old_hash: String,
    /// New block hash at the fork point.
    pub new_hash: String,
    /// Timestamp of detection.
    pub detected_at: u64,
}

impl ReorgDetector {
    /// Create a new reorganization detector.
    pub fn new() -> Self {
        Self {
            known_blocks: FxHashMap::default(),
            reorgs: Vec::new(),
            max_reorg_depth: 0,
        }
    }

    /// Record a block at a given height.
    ///
    /// If a different block was previously recorded at the same height, a
    /// reorganization is detected and recorded.
    pub fn record_block(
        &mut self,
        height: u64,
        hash: String,
        timestamp: u64,
    ) -> Option<ReorgEvent> {
        if let Some(existing_hash) = self.known_blocks.get(&height) {
            if *existing_hash != hash {
                // Reorganization detected
                let event = ReorgEvent {
                    fork_height: height,
                    depth: 1, // minimum depth; caller may provide more context
                    old_hash: existing_hash.clone(),
                    new_hash: hash.clone(),
                    detected_at: timestamp,
                };

                self.reorgs.push(event.clone());
                self.known_blocks.insert(height, hash);

                return Some(event);
            }
        } else {
            self.known_blocks.insert(height, hash);
        }

        None
    }

    /// Record a reorganization of a specific depth.
    pub fn record_reorg(
        &mut self,
        fork_height: u64,
        depth: u64,
        old_hash: String,
        new_hash: String,
        timestamp: u64,
    ) {
        if depth > self.max_reorg_depth {
            self.max_reorg_depth = depth;
        }

        let event = ReorgEvent {
            fork_height,
            depth,
            old_hash: old_hash.clone(),
            new_hash: new_hash.clone(),
            detected_at: timestamp,
        };

        self.reorgs.push(event);

        // Update known blocks from fork_height onward
        self.known_blocks.insert(fork_height, new_hash);
    }

    /// Get the maximum reorganization depth observed.
    pub fn max_reorg_depth(&self) -> u64 {
        self.max_reorg_depth
    }

    /// Get all recorded reorganization events.
    pub fn reorg_history(&self) -> &[ReorgEvent] {
        &self.reorgs
    }

    /// Calculate an adjusted finality depth recommendation based on observed reorgs.
    ///
    /// Returns a depth that is at least 2x the maximum observed reorg depth,
    /// with a minimum of the provided base depth.
    pub fn recommended_finality_depth(&self, base_depth: u64) -> u64 {
        let safety_depth = self.max_reorg_depth * 2 + 1;
        base_depth.max(safety_depth)
    }
}

impl Default for ReorgDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Manages the creation and validation of finality checkpoints.
#[derive(Debug, Clone)]
pub struct CheckpointManager {
    /// Interval (in blocks) between periodic checkpoints.
    checkpoint_interval: u64,
    /// Minimum number of validator signatures required for a valid checkpoint.
    required_signatures: usize,
    /// Minimum total stake weight required across all signatures.
    required_stake_weight: u64,
    /// All recorded checkpoints indexed by block height.
    checkpoints: FxHashMap<u64, FinalityCheckpoint>,
    /// The height of the most recent checkpoint.
    latest_checkpoint_height: u64,
}

impl CheckpointManager {
    /// Create a new checkpoint manager.
    pub fn new(
        checkpoint_interval: u64,
        required_signatures: usize,
        required_stake_weight: u64,
    ) -> Self {
        Self {
            checkpoint_interval,
            required_signatures,
            required_stake_weight,
            checkpoints: FxHashMap::default(),
            latest_checkpoint_height: 0,
        }
    }

    /// Check if a periodic checkpoint should be created at the given height.
    pub fn should_create_periodic_checkpoint(&self, block_height: u64) -> bool {
        block_height > 0 && block_height % self.checkpoint_interval == 0
    }

    /// Create a checkpoint at the given block height.
    ///
    /// Validates that sufficient validator signatures are present and that
    /// their combined stake weight meets the threshold.
    pub fn create_checkpoint(
        &mut self,
        block_hash: String,
        block_height: u64,
        signatures: Vec<ValidatorSignature>,
        checkpoint_type: CheckpointType,
        timestamp: u64,
    ) -> Result<FinalityCheckpoint, FinalityError> {
        if self.checkpoints.contains_key(&block_height) {
            return Err(FinalityError::CheckpointAlreadyExists(block_height));
        }

        if signatures.len() < self.required_signatures {
            return Err(FinalityError::InsufficientSignatures {
                got: signatures.len(),
                need: self.required_signatures,
            });
        }

        let total_stake: u64 = signatures.iter().map(|s| s.stake_weight).sum();
        if total_stake < self.required_stake_weight {
            return Err(FinalityError::InsufficientSignatures {
                got: signatures.len(),
                need: self.required_signatures,
            });
        }

        let checkpoint_hash = FinalityCheckpoint::compute_hash(
            &block_hash,
            block_height,
            &checkpoint_type,
            timestamp,
        );

        let checkpoint = FinalityCheckpoint {
            block_hash,
            block_height,
            validator_signatures: signatures,
            checkpoint_type,
            timestamp,
            checkpoint_hash,
        };

        self.checkpoints.insert(block_height, checkpoint.clone());

        if block_height > self.latest_checkpoint_height {
            self.latest_checkpoint_height = block_height;
        }

        Ok(checkpoint)
    }

    /// Get the checkpoint at a specific height, if it exists.
    pub fn get_checkpoint(&self, height: u64) -> Option<&FinalityCheckpoint> {
        self.checkpoints.get(&height)
    }

    /// Get the most recent checkpoint.
    pub fn latest_checkpoint(&self) -> Option<&FinalityCheckpoint> {
        self.checkpoints.get(&self.latest_checkpoint_height)
    }

    /// Get the height of the latest checkpoint.
    pub fn latest_checkpoint_height(&self) -> u64 {
        self.latest_checkpoint_height
    }

    /// Get all checkpoints.
    pub fn all_checkpoints(&self) -> &FxHashMap<u64, FinalityCheckpoint> {
        &self.checkpoints
    }

    /// Check if a block at the given height is covered by a checkpoint
    /// (i.e., at or below a checkpoint height).
    pub fn is_checkpointed(&self, block_height: u64) -> bool {
        self.checkpoints
            .keys()
            .any(|&cp_height| cp_height >= block_height)
    }
}

/// Unified finality oracle combining probabilistic, economic, and checkpoint finality.
///
/// Answers the question: "What is the probability this block is final?"
/// Returns 1.0 for blocks covered by a BFT checkpoint, and a probabilistic
/// estimate for blocks beyond the latest checkpoint.
#[derive(Debug, Clone)]
pub struct FinalityOracle {
    confirmation_probability: ConfirmationProbability,
    economic_finality: EconomicFinality,
    checkpoint_manager: CheckpointManager,
    reorg_detector: ReorgDetector,
    /// Current chain tip height.
    chain_tip_height: u64,
}

impl FinalityOracle {
    /// Create a new finality oracle.
    pub fn new(
        confirmation_params: ConfirmationParams,
        economic_finality: EconomicFinality,
        checkpoint_manager: CheckpointManager,
    ) -> Self {
        Self {
            confirmation_probability: ConfirmationProbability::new(confirmation_params),
            economic_finality,
            checkpoint_manager,
            reorg_detector: ReorgDetector::new(),
            chain_tip_height: 0,
        }
    }

    /// Update the chain tip height.
    pub fn set_chain_tip(&mut self, height: u64) {
        self.chain_tip_height = height;
    }

    /// Query the finality probability for a block at a given height.
    ///
    /// Returns 1.0 if the block is covered by a BFT checkpoint.
    /// Otherwise returns a probabilistic estimate based on depth.
    pub fn finality_probability(&self, block_height: u64) -> f64 {
        // If the block is at or below a checkpoint, it has absolute finality
        if self.checkpoint_manager.is_checkpointed(block_height)
            && block_height <= self.checkpoint_manager.latest_checkpoint_height()
        {
            return 1.0;
        }

        // Calculate depth-based probabilistic finality
        let depth = self.chain_tip_height.saturating_sub(block_height);
        self.confirmation_probability.calculate(depth, None)
    }

    /// Query finality probability with economic considerations.
    ///
    /// Combines probabilistic finality with economic finality to give a
    /// comprehensive finality score.
    pub fn comprehensive_finality(&self, block_height: u64, value_at_risk: u64) -> f64 {
        let prob_finality = self.finality_probability(block_height);
        let econ_finality = self
            .economic_finality
            .economic_finality_score(value_at_risk);

        // Take the maximum of probabilistic and economic finality.
        // If either indicates finality, the block should be considered final.
        prob_finality.max(econ_finality)
    }

    /// Record a new block and check for reorganizations.
    pub fn record_block(
        &mut self,
        height: u64,
        hash: String,
        timestamp: u64,
    ) -> Option<ReorgEvent> {
        if height > self.chain_tip_height {
            self.chain_tip_height = height;
        }
        self.reorg_detector.record_block(height, hash, timestamp)
    }

    /// Get the reorg detector for inspection.
    pub fn reorg_detector(&self) -> &ReorgDetector {
        &self.reorg_detector
    }

    /// Get the checkpoint manager for inspection.
    pub fn checkpoint_manager(&self) -> &CheckpointManager {
        &self.checkpoint_manager
    }

    /// Get a mutable reference to the checkpoint manager.
    pub fn checkpoint_manager_mut(&mut self) -> &mut CheckpointManager {
        &mut self.checkpoint_manager
    }

    /// Get the recommended finality depth based on observed network behavior.
    pub fn recommended_finality_depth(&self, base_depth: u64) -> u64 {
        self.reorg_detector.recommended_finality_depth(base_depth)
    }
}

/// Top-level finality gadget combining all finality subsystems.
///
/// Provides a single entry point for all finality-related operations:
/// probabilistic confidence, BFT checkpoints, economic finality, and
/// reorganization monitoring.
pub struct FinalityGadget {
    oracle: FinalityOracle,
}

impl FinalityGadget {
    /// Create a new finality gadget with the given configuration.
    pub fn new(
        confirmation_params: ConfirmationParams,
        total_staked: u64,
        slashing_fraction: f64,
        validator_count: usize,
        checkpoint_interval: u64,
        required_checkpoint_signatures: usize,
        required_stake_weight: u64,
    ) -> Self {
        // BFT requires > 2/3 of validators to be honest, so min colluding is 1/3 + 1
        let min_colluding = validator_count / 3 + 1;

        let economic = EconomicFinality::new(total_staked, slashing_fraction, min_colluding);
        let checkpoint_mgr = CheckpointManager::new(
            checkpoint_interval,
            required_checkpoint_signatures,
            required_stake_weight,
        );

        Self {
            oracle: FinalityOracle::new(confirmation_params, economic, checkpoint_mgr),
        }
    }

    /// Get the finality probability for a block.
    pub fn query_finality(&self, block_height: u64) -> f64 {
        self.oracle.finality_probability(block_height)
    }

    /// Get comprehensive finality score including economic analysis.
    pub fn query_comprehensive_finality(&self, block_height: u64, value_at_risk: u64) -> f64 {
        self.oracle
            .comprehensive_finality(block_height, value_at_risk)
    }

    /// Update the chain tip.
    pub fn set_chain_tip(&mut self, height: u64) {
        self.oracle.set_chain_tip(height);
    }

    /// Record a new block and detect potential reorganizations.
    pub fn record_block(
        &mut self,
        height: u64,
        hash: String,
        timestamp: u64,
    ) -> Option<ReorgEvent> {
        self.oracle.record_block(height, hash, timestamp)
    }

    /// Create a finality checkpoint.
    pub fn create_checkpoint(
        &mut self,
        block_hash: String,
        block_height: u64,
        signatures: Vec<ValidatorSignature>,
        checkpoint_type: CheckpointType,
        timestamp: u64,
    ) -> Result<FinalityCheckpoint, FinalityError> {
        self.oracle.checkpoint_manager_mut().create_checkpoint(
            block_hash,
            block_height,
            signatures,
            checkpoint_type,
            timestamp,
        )
    }

    /// Check if a periodic checkpoint should be created at this height.
    pub fn should_checkpoint(&self, block_height: u64) -> bool {
        self.oracle
            .checkpoint_manager()
            .should_create_periodic_checkpoint(block_height)
    }

    /// Get the oracle for direct access to finality subsystems.
    pub fn oracle(&self) -> &FinalityOracle {
        &self.oracle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_signatures(count: usize, stake_each: u64) -> Vec<ValidatorSignature> {
        (0..count)
            .map(|i| ValidatorSignature {
                validator_address: format!("validator-{}", i),
                signature: vec![i as u8; 64],
                stake_weight: stake_each,
            })
            .collect()
    }

    #[test]
    fn test_finality_increases_with_depth() {
        let calc = ConfirmationProbability::new(ConfirmationParams::default());

        let p1 = calc.calculate(1, None);
        let p5 = calc.calculate(5, None);
        let p10 = calc.calculate(10, None);
        let p30 = calc.calculate(30, None);

        assert!(p1 > 0.0, "depth 1 should have some finality");
        assert!(p5 > p1, "depth 5 should be more final than depth 1");
        assert!(p10 > p5, "depth 10 should be more final than depth 5");
        assert!(p30 > p10, "depth 30 should be more final than depth 10");
        assert!(
            p30 > 0.99,
            "depth 30 with default params should be near-certain"
        );
    }

    #[test]
    fn test_finality_zero_at_depth_zero() {
        let calc = ConfirmationProbability::new(ConfirmationParams::default());
        assert_eq!(calc.calculate(0, None), 0.0);
    }

    #[test]
    fn test_finality_zero_with_majority_adversary() {
        let calc = ConfirmationProbability::new(ConfirmationParams::default());
        assert_eq!(calc.calculate(100, Some(0.5)), 0.0);
    }

    #[test]
    fn test_finality_higher_with_lower_adversary() {
        let calc = ConfirmationProbability::new(ConfirmationParams::default());
        let p_low_adversary = calc.calculate(5, Some(0.1));
        let p_high_adversary = calc.calculate(5, Some(0.4));

        assert!(
            p_low_adversary > p_high_adversary,
            "lower adversary threshold should yield higher finality"
        );
    }

    #[test]
    fn test_checkpoint_creation() {
        let mut mgr = CheckpointManager::new(100, 3, 300);
        let sigs = make_signatures(3, 150);

        let cp = mgr
            .create_checkpoint(
                "block-hash-100".to_string(),
                100,
                sigs,
                CheckpointType::Periodic,
                1000,
            )
            .unwrap();

        assert_eq!(cp.block_height, 100);
        assert_eq!(cp.checkpoint_type, CheckpointType::Periodic);
        assert_eq!(cp.validator_signatures.len(), 3);
        assert!(!cp.checkpoint_hash.is_empty());
    }

    #[test]
    fn test_checkpoint_duplicate_rejected() {
        let mut mgr = CheckpointManager::new(100, 2, 200);
        let sigs = make_signatures(3, 150);

        mgr.create_checkpoint(
            "hash-100".to_string(),
            100,
            sigs.clone(),
            CheckpointType::Periodic,
            1000,
        )
        .unwrap();

        let result = mgr.create_checkpoint(
            "hash-100-b".to_string(),
            100,
            sigs,
            CheckpointType::Periodic,
            1001,
        );

        assert!(matches!(
            result,
            Err(FinalityError::CheckpointAlreadyExists(100))
        ));
    }

    #[test]
    fn test_checkpoint_insufficient_signatures() {
        let mut mgr = CheckpointManager::new(100, 5, 100);
        let sigs = make_signatures(3, 100);

        let result = mgr.create_checkpoint(
            "hash-100".to_string(),
            100,
            sigs,
            CheckpointType::Periodic,
            1000,
        );

        assert!(matches!(
            result,
            Err(FinalityError::InsufficientSignatures { got: 3, need: 5 })
        ));
    }

    #[test]
    fn test_absolute_finality_after_checkpoint() {
        let params = ConfirmationParams::default();
        let economic = EconomicFinality::new(1_000_000, 0.1, 5);
        let mut checkpoint_mgr = CheckpointManager::new(100, 2, 100);

        let sigs = make_signatures(3, 100);
        checkpoint_mgr
            .create_checkpoint(
                "hash-100".to_string(),
                100,
                sigs,
                CheckpointType::Periodic,
                1000,
            )
            .unwrap();

        let mut oracle = FinalityOracle::new(params, economic, checkpoint_mgr);
        oracle.set_chain_tip(150);

        // Block at height 50 is below checkpoint at 100 -> absolute finality
        assert_eq!(oracle.finality_probability(50), 1.0);

        // Block at height 100 is at the checkpoint -> absolute finality
        assert_eq!(oracle.finality_probability(100), 1.0);

        // Block at height 130 is above checkpoint -> probabilistic finality
        let p130 = oracle.finality_probability(130);
        assert!(p130 < 1.0);
        assert!(p130 > 0.0);
    }

    #[test]
    fn test_periodic_checkpoint_schedule() {
        let mgr = CheckpointManager::new(100, 2, 100);

        assert!(!mgr.should_create_periodic_checkpoint(0));
        assert!(!mgr.should_create_periodic_checkpoint(50));
        assert!(mgr.should_create_periodic_checkpoint(100));
        assert!(!mgr.should_create_periodic_checkpoint(150));
        assert!(mgr.should_create_periodic_checkpoint(200));
        assert!(mgr.should_create_periodic_checkpoint(300));
    }

    #[test]
    fn test_reorg_detection() {
        let mut detector = ReorgDetector::new();

        assert!(
            detector
                .record_block(1, "hash-a".to_string(), 100)
                .is_none()
        );
        assert!(
            detector
                .record_block(2, "hash-b".to_string(), 200)
                .is_none()
        );

        // Reorg at height 2
        let event = detector.record_block(2, "hash-c".to_string(), 300);
        assert!(event.is_some());

        let reorg = event.unwrap();
        assert_eq!(reorg.fork_height, 2);
        assert_eq!(reorg.old_hash, "hash-b");
        assert_eq!(reorg.new_hash, "hash-c");
    }

    #[test]
    fn test_reorg_depth_tracking() {
        let mut detector = ReorgDetector::new();

        detector.record_reorg(50, 3, "old".to_string(), "new".to_string(), 1000);
        assert_eq!(detector.max_reorg_depth(), 3);

        detector.record_reorg(80, 7, "old2".to_string(), "new2".to_string(), 2000);
        assert_eq!(detector.max_reorg_depth(), 7);

        // Smaller reorg should not reduce the max
        detector.record_reorg(90, 2, "old3".to_string(), "new3".to_string(), 3000);
        assert_eq!(detector.max_reorg_depth(), 7);
    }

    #[test]
    fn test_recommended_finality_depth() {
        let mut detector = ReorgDetector::new();

        assert_eq!(detector.recommended_finality_depth(10), 10);

        detector.record_reorg(50, 3, "old".to_string(), "new".to_string(), 1000);
        assert_eq!(detector.recommended_finality_depth(5), 7);

        assert_eq!(detector.recommended_finality_depth(20), 20);
    }

    #[test]
    fn test_economic_finality_cost_to_revert() {
        let ef = EconomicFinality::new(1_000_000, 0.10, 4);
        let cost = ef.cost_to_revert();
        assert_eq!(cost, 100_000);
    }

    #[test]
    fn test_economic_finality_score() {
        let ef = EconomicFinality::new(1_000_000, 0.10, 4);

        assert_eq!(ef.economic_finality_score(50_000), 1.0);
        assert!((ef.economic_finality_score(200_000) - 0.5).abs() < 0.001);
        assert_eq!(ef.economic_finality_score(0), 1.0);
    }

    #[test]
    fn test_finality_gadget_integration() {
        let params = ConfirmationParams {
            adversary_threshold: 0.25,
            total_network_weight: 1_000_000,
            base_confidence_per_depth: 0.15,
        };

        let mut gadget = FinalityGadget::new(
            params, 1_000_000, // total staked
            0.10,      // slashing fraction
            12,        // validator count
            100,       // checkpoint interval
            3,         // required signatures
            300,       // required stake weight
        );

        // Record blocks
        for i in 0..=200 {
            gadget.record_block(i, format!("hash-{}", i), i * 10);
        }
        gadget.set_chain_tip(200);

        // Block near the tip (depth=1) without checkpoint: probabilistic finality
        // With adversary_threshold=0.25, q=0.333, depth=1: finality ~= 0.667
        let p_near_tip = gadget.query_finality(199);
        assert!(p_near_tip > 0.0, "finality at depth 1 should be > 0");
        assert!(
            p_near_tip < 1.0,
            "finality at depth 1 should be < 1.0 (got {})",
            p_near_tip
        );

        // Block deep in the chain (depth=100) converges to 1.0
        let p_deep = gadget.query_finality(100);
        assert!(p_deep > 0.99, "finality at depth 100 should approach 1.0");

        // Create checkpoint at height 100
        let sigs = make_signatures(4, 100);
        gadget
            .create_checkpoint(
                "hash-100".to_string(),
                100,
                sigs,
                CheckpointType::Periodic,
                1000,
            )
            .unwrap();

        assert_eq!(gadget.query_finality(100), 1.0);
        assert_eq!(gadget.query_finality(50), 1.0);

        // Block at depth 2: finality should be between 0 and 1
        let p198 = gadget.query_finality(198);
        assert!(p198 > 0.0);
        assert!(p198 < 1.0);
    }

    #[test]
    fn test_finality_with_weight() {
        let calc = ConfirmationProbability::new(ConfirmationParams {
            adversary_threshold: 0.33,
            total_network_weight: 1_000,
            base_confidence_per_depth: 0.15,
        });

        let p_low_weight = calc.calculate_with_weight(5, 100);
        let p_high_weight = calc.calculate_with_weight(5, 900);

        assert!(
            p_high_weight > p_low_weight,
            "higher weight should increase confidence"
        );
    }

    #[test]
    fn test_checkpoint_should_create_periodic() {
        let gadget = FinalityGadget::new(
            ConfirmationParams::default(),
            1_000_000,
            0.10,
            12,
            50,
            2,
            100,
        );

        assert!(gadget.should_checkpoint(50));
        assert!(gadget.should_checkpoint(100));
        assert!(!gadget.should_checkpoint(75));
        assert!(!gadget.should_checkpoint(0));
    }

    #[test]
    fn test_emergency_checkpoint() {
        let mut mgr = CheckpointManager::new(100, 2, 100);
        let sigs = make_signatures(3, 100);

        let cp = mgr
            .create_checkpoint(
                "emergency-hash".to_string(),
                42,
                sigs,
                CheckpointType::Emergency,
                999,
            )
            .unwrap();

        assert_eq!(cp.checkpoint_type, CheckpointType::Emergency);
        assert_eq!(cp.block_height, 42);
        assert!(mgr.is_checkpointed(42));
        assert!(mgr.is_checkpointed(30));
    }

    #[test]
    fn test_comprehensive_finality() {
        let params = ConfirmationParams::default();
        let economic = EconomicFinality::new(10_000_000, 0.5, 10);
        let checkpoint_mgr = CheckpointManager::new(100, 2, 100);

        let mut oracle = FinalityOracle::new(params, economic, checkpoint_mgr);
        oracle.set_chain_tip(5);

        let prob_only = oracle.finality_probability(4);
        let comprehensive = oracle.comprehensive_finality(4, 100_000);

        assert!(
            comprehensive >= prob_only,
            "comprehensive should be at least as high as probabilistic"
        );
    }
}
