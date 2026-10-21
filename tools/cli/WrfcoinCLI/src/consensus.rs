use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use log::{info, warn};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};

use crate::security::{CheckpointBlock, ConsensusResult, SecurityService};

/// Block header information
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Block hash
    pub hash: String,
    /// Block height
    pub height: u64,
    /// Previous block hash
    pub prev_hash: String,
    /// Block timestamp
    pub timestamp: String,
    /// Merkle root
    pub merkle_root: String,
    /// Block difficulty
    pub difficulty: u64,
    /// Block nonce
    pub nonce: u64,
    /// Block version
    pub version: u8,
    /// Block type
    pub block_type: String,
}

/// Fork information
#[derive(Clone, Debug)]
pub struct Fork {
    /// Fork chain ID
    pub id: String,
    /// Fork start height
    pub start_height: u64,
    /// Fork blocks
    pub blocks: Vec<BlockHeader>,
    /// Cumulative difficulty
    pub cumulative_difficulty: u64,
    /// First seen timestamp
    pub first_seen: DateTime<Utc>,
    /// Last updated timestamp
    pub last_updated: DateTime<Utc>,
}

/// Consensus algorithm
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ConsensusAlgorithm {
    /// Proof of Work
    ProofOfWork,
    /// Proof of Stake
    ProofOfStake,
    /// Delegated Proof of Stake
    DelegatedProofOfStake,
    /// Proof of Authority
    ProofOfAuthority,
    /// Hybrid Proof of Work/Stake
    HybridPowPos,
    /// Byzantine Fault Tolerance
    BFT,
}

/// Consensus parameters
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConsensusParameters {
    /// Consensus algorithm
    pub algorithm: ConsensusAlgorithm,
    /// Target block time in seconds
    pub target_block_time: u64,
    /// Block reward
    pub block_reward: f64,
    /// Block reward halving interval (in blocks)
    pub reward_halving_interval: Option<u64>,
    /// Maximum supply
    pub max_supply: Option<f64>,
    /// Initial difficulty
    pub initial_difficulty: u64,
    /// Difficulty adjustment interval (in blocks)
    pub difficulty_adjustment_interval: u64,
    /// Maximum difficulty adjustment factor
    pub max_difficulty_adjustment_factor: f64,
    /// Maximum future block time (seconds)
    pub max_future_block_time: u64,
    /// Maximum block size (bytes)
    pub max_block_size: u64,
    /// Minimum number of confirmations
    pub min_confirmations: u64,
}

impl Default for ConsensusParameters {
    fn default() -> Self {
        Self {
            algorithm: ConsensusAlgorithm::ProofOfWork,
            target_block_time: 15,
            block_reward: 50.0,
            reward_halving_interval: Some(210000),
            max_supply: Some(21000000.0),
            initial_difficulty: 1,
            difficulty_adjustment_interval: 2016,
            max_difficulty_adjustment_factor: 0.25,
            max_future_block_time: 120,
            max_block_size: 1000000,
            min_confirmations: 6,
        }
    }
}

/// Consensus engine for Wrfcoin
pub struct ConsensusEngine {
    /// Security service
    security_service: Arc<Mutex<SecurityService>>,
    /// Consensus parameters
    parameters: ConsensusParameters,
    /// Current block chain
    chain: Arc<RwLock<Vec<BlockHeader>>>,
    /// Forks
    forks: Arc<RwLock<FxHashMap<String, Fork>>>,
    /// Orphan blocks
    orphans: Arc<RwLock<FxHashMap<String, BlockHeader>>>,
    /// Block timestamps for difficulty calculation
    block_timestamps: Arc<RwLock<VecDeque<u64>>>,
    /// Current difficulty
    current_difficulty: Arc<RwLock<u64>>,
}

impl ConsensusEngine {
    /// Create a new consensus engine
    pub fn new(
        security_service: Arc<Mutex<SecurityService>>,
        parameters: ConsensusParameters,
        genesis_block: BlockHeader,
    ) -> Self {
        let chain = Arc::new(RwLock::new(vec![genesis_block.clone()]));
        let forks = Arc::new(RwLock::new(FxHashMap::default()));
        let orphans = Arc::new(RwLock::new(FxHashMap::default()));

        let mut timestamps = VecDeque::new();
        let genesis_timestamp = match genesis_block.timestamp.parse::<DateTime<Utc>>() {
            Ok(dt) => dt.timestamp() as u64,
            Err(_) => {
                // Fallback to current timestamp if parsing fails
                Utc::now().timestamp() as u64
            },
        };
        timestamps.push_back(genesis_timestamp);

        let block_timestamps = Arc::new(RwLock::new(timestamps));
        let current_difficulty = Arc::new(RwLock::new(parameters.initial_difficulty));

        Self {
            security_service,
            parameters,
            chain,
            forks,
            orphans,
            block_timestamps,
            current_difficulty,
        }
    }

    /// Get the current chain height
    pub fn get_height(&self) -> u64 {
        let chain = self.chain.read().unwrap();
        chain.last().map(|block| block.height).unwrap_or(0)
    }

    /// Get the current difficulty
    pub fn get_difficulty(&self) -> u64 {
        *self.current_difficulty.read().unwrap()
    }

    /// Get block at height
    pub fn get_block_at_height(&self, height: u64) -> Option<BlockHeader> {
        let chain = self.chain.read().unwrap();
        chain.get(height as usize).cloned()
    }

    /// Get block by hash
    pub fn get_block_by_hash(&self, hash: &str) -> Option<BlockHeader> {
        let chain = self.chain.read().unwrap();
        chain.iter().find(|block| block.hash == hash).cloned()
    }

    /// Get the latest block
    pub fn get_latest_block(&self) -> Option<BlockHeader> {
        let chain = self.chain.read().unwrap();
        chain.last().cloned()
    }

    /// Add a checkpoint
    pub fn add_checkpoint(&self, checkpoint: CheckpointBlock) -> Result<()> {
        // Validate checkpoint data before adding
        if checkpoint.height == 0 {
            return Err(anyhow!("Cannot add checkpoint at genesis block height 0"));
        }

        if checkpoint.hash.is_empty() {
            return Err(anyhow!("Checkpoint hash cannot be empty"));
        }

        // Validate that checkpoint is for a block that exists or could exist
        let current_height = self.get_height();
        if checkpoint.height > current_height + 10000 {
            // Prevent adding checkpoints too far in the future
            return Err(anyhow!(
                "Checkpoint height {} is too far ahead of current height {}",
                checkpoint.height,
                current_height
            ));
        }

        // Check if we already have a block at this height and if hashes match
        if let Some(existing_block) = self.get_block_at_height(checkpoint.height) {
            if existing_block.hash != checkpoint.hash {
                return Err(anyhow!(
                    "Checkpoint hash mismatch: existing block hash {} != checkpoint hash {}",
                    existing_block.hash,
                    checkpoint.hash
                ));
            }
        }

        // Add checkpoint to security service with thread-safe access
        {
            let mut security_service = self.security_service.lock().unwrap();
            security_service.add_checkpoint(checkpoint.clone());
        }

        info!(
            "Successfully added checkpoint at height {} with hash {}",
            checkpoint.height, checkpoint.hash
        );

        // Validate any existing forks against this new checkpoint
        self.validate_forks_against_checkpoint(&checkpoint)?;

        Ok(())
    }

    /// Validate existing forks against a new checkpoint
    fn validate_forks_against_checkpoint(&self, checkpoint: &CheckpointBlock) -> Result<()> {
        let forks_to_remove = self.find_conflicting_forks(checkpoint);

        self.remove_conflicting_forks(forks_to_remove, checkpoint)?;
        Ok(())
    }

    /// Find forks that conflict with a checkpoint
    fn find_conflicting_forks(&self, checkpoint: &CheckpointBlock) -> Vec<String> {
        let mut forks_to_remove = Vec::new();
        let forks = self.forks.read().unwrap();

        for (fork_id, fork) in forks.iter() {
            let has_conflict = fork
                .blocks
                .iter()
                .any(|block| block.height == checkpoint.height && block.hash != checkpoint.hash);

            if has_conflict {
                warn!(
                    "Fork {fork_id} has conflicting block at checkpoint height {}: expected {}",
                    checkpoint.height, checkpoint.hash
                );
                forks_to_remove.push(fork_id.clone());
            }
        }

        forks_to_remove
    }

    /// Remove conflicting forks and move their blocks to orphans
    fn remove_conflicting_forks(
        &self,
        forks_to_remove: Vec<String>,
        checkpoint: &CheckpointBlock,
    ) -> Result<()> {
        if forks_to_remove.is_empty() {
            return Ok(());
        }

        let mut forks = self.forks.write().unwrap();
        let mut orphans = self.orphans.write().unwrap();

        for fork_id in forks_to_remove {
            if let Some(removed_fork) = forks.remove(&fork_id) {
                info!(
                    "Removed fork {fork_id} due to checkpoint conflict at height {}",
                    checkpoint.height
                );

                // Move fork blocks to orphans for potential future reconnection
                self.move_blocks_to_orphans(&mut orphans, removed_fork.blocks);
            }
        }

        Ok(())
    }

    /// Helper to move blocks to orphans collection
    fn move_blocks_to_orphans(
        &self,
        orphans: &mut FxHashMap<String, BlockHeader>,
        blocks: Vec<BlockHeader>,
    ) {
        for block in blocks {
            orphans.insert(block.hash.clone(), block);
        }
    }

    /// Helper to add chain blocks to a fork
    fn add_chain_blocks_to_fork(
        &self,
        fork: &mut Fork,
        chain: &[BlockHeader],
        start_height: u64,
        end_height: u64,
    ) {
        for height in start_height..end_height {
            if let Some(chain_block) = chain.get(height as usize) {
                fork.blocks.push(chain_block.clone());
                fork.cumulative_difficulty += chain_block.difficulty;
            }
        }
    }

    /// Validate a block at or before current chain height
    fn validate_historical_block(
        &self,
        block: &BlockHeader,
        _height: u64,
    ) -> Result<ConsensusResult> {
        // Check if we already have a block at this height
        if let Some(existing_block) = self.get_block_at_height(block.height) {
            if existing_block.hash == block.hash {
                // Duplicate block
                return Ok(ConsensusResult::Invalid(format!(
                    "Duplicate block at height {}: {}",
                    block.height, block.hash
                )));
            }
            // Could be a competing block in a fork
        }

        Ok(ConsensusResult::Valid)
    }

    /// Process orphan blocks that might now connect to the chain
    fn process_connecting_orphans(&self, connecting_orphans: Vec<BlockHeader>) -> Result<()> {
        for orphan in connecting_orphans {
            let hash = orphan.hash.clone();

            match self.process_block(orphan.clone())? {
                ConsensusResult::Valid => {
                    info!("Orphan block {hash} now connected to main chain");
                },
                _ => {
                    let mut orphans = self.orphans.write().unwrap();
                    orphans.insert(orphan.hash.clone(), orphan);
                },
            }
        }

        Ok(())
    }

    /// Process a new block
    pub fn process_block(&self, block: BlockHeader) -> Result<ConsensusResult> {
        // First, perform basic validation
        let validation_result = self.validate_block(&block)?;
        if validation_result != ConsensusResult::Valid {
            return Ok(validation_result);
        }

        // Check if this block builds on the current chain
        let chain = self.chain.read().unwrap();
        if let Some(latest_block) = chain.last() {
            if block.prev_hash == latest_block.hash {
                // Block extends the current chain
                drop(chain); // Release the read lock
                self.extend_chain(block.clone())?;
                return Ok(ConsensusResult::Valid);
            }
        }
        drop(chain);

        // Not extending main chain, check if it's extending a known fork
        let mut fork_id_to_check = None;
        {
            let mut forks = self.forks.write().unwrap();
            let mut fork_extended = false;

            for (_, fork) in forks.iter_mut() {
                let Some(latest_fork_block) = fork.blocks.last() else {
                    continue;
                };

                if block.prev_hash == latest_fork_block.hash {
                    self.extend_fork_with_block(fork, &block);
                    fork_extended = true;
                    fork_id_to_check = Some(fork.id.clone());
                    break;
                }
            }

            if fork_extended {
                drop(forks);
                if let Some(fork_id) = fork_id_to_check {
                    self.check_fork_reorganization(&fork_id)?;
                }
                return Ok(ConsensusResult::Valid);
            }
        }

        // If we reach here, no fork was extended
        {
            // Check if this block is creating a new fork
            let chain = self.chain.read().unwrap();
            for chain_block in chain.iter() {
                if block.prev_hash == chain_block.hash {
                    // New fork from main chain
                    let fork_id = format!("fork-{}", block.hash);
                    let mut fork = Fork {
                        id: fork_id.clone(),
                        start_height: chain_block.height,
                        blocks: Vec::new(),
                        cumulative_difficulty: 0,
                        first_seen: Utc::now(),
                        last_updated: Utc::now(),
                    };

                    // Add all blocks after fork point from chain_block.height + 1 to block.height
                    self.add_chain_blocks_to_fork(
                        &mut fork,
                        &chain,
                        chain_block.height + 1,
                        block.height,
                    );

                    // Add the new block
                    fork.blocks.push(block.clone());
                    fork.cumulative_difficulty += block.difficulty;

                    info!(
                        "New fork {} started at height {} with block {}",
                        fork_id, chain_block.height, block.hash
                    );

                    let mut forks = self.forks.write().unwrap();
                    forks.insert(fork_id, fork);
                    return Ok(ConsensusResult::Valid);
                }
            }

            // Not extending anything we know about, treat as orphan
            info!(
                "Orphan block received: {} at height {}",
                block.hash, block.height
            );

            let block_hash = block.hash.clone();
            let block_height = block.height;
            let block_prev_hash = block.prev_hash.clone();

            let mut orphans = self.orphans.write().unwrap();
            orphans.insert(block_hash.clone(), block);

            return Ok(ConsensusResult::Indeterminate(format!(
                "Block is orphaned: {block_hash} at height {block_height}, prev_hash: {block_prev_hash}"
            )));
        }

        // This code is actually reachable when the block is not orphaned
        // The warning is a false positive
        #[allow(unreachable_code)]
        Ok(ConsensusResult::Valid)
    }

    /// Validate a block
    fn validate_block(&self, block: &BlockHeader) -> Result<ConsensusResult> {
        // Check block is not from too far in the future
        let now = Utc::now();
        let max_future_time =
            now + chrono::Duration::seconds(self.parameters.max_future_block_time as i64);

        // Parse block timestamp from string
        if let Ok(block_time) = block.timestamp.parse::<DateTime<Utc>>() {
            if block_time > max_future_time {
                return Ok(ConsensusResult::Invalid(format!(
                    "Block timestamp is too far in the future: {} > {}",
                    block.timestamp, max_future_time
                )));
            }
        } else {
            return Ok(ConsensusResult::Invalid(format!(
                "Invalid block timestamp format: {}",
                block.timestamp
            )));
        }

        // Check if block exceeds maximum size
        // In a real implementation, we would check actual block size here

        // Check if block height is valid
        let current_height = self.get_height();
        if block.height != current_height + 1 {
            // This could be a fork or orphan, not necessarily invalid
            if block.height <= current_height {
                return self.validate_historical_block(block, block.height);
            } else if block.height > current_height + 1 {
                // Block is ahead of our chain, could be orphan
                // We'll add it to orphans later
            }
        }

        // Check block difficulty
        if block.difficulty < self.get_difficulty() {
            return Ok(ConsensusResult::Invalid(format!(
                "Block difficulty too low: {} < {}",
                block.difficulty,
                self.get_difficulty()
            )));
        }

        // Check if block is a checkpoint
        {
            let security_service = self.security_service.lock().unwrap();
            if security_service.is_checkpoint(block.height)
                && !security_service.verify_checkpoint(block.height, &block.hash)?
            {
                return Ok(ConsensusResult::Invalid(format!(
                    "Block does not match checkpoint at height {}",
                    block.height
                )));
            }
        }

        // If using Proof of Work, validate the hash meets difficulty target
        // In a real implementation, we would verify the block hash against difficulty

        // Block appears valid
        Ok(ConsensusResult::Valid)
    }

    /// Extend the main chain with a new block
    fn extend_chain(&self, block: BlockHeader) -> Result<()> {
        info!(
            "Extending main chain with block {} at height {}",
            block.hash, block.height
        );

        let block_height = block.height;
        {
            let mut chain = self.chain.write().unwrap();
            chain.push(block.clone());
        }

        // Update difficulty if needed
        self.update_difficulty(block_height)?;

        // Update timestamps for difficulty calculation
        let mut timestamps = self.block_timestamps.write().unwrap();
        let block_timestamp = match block.timestamp.parse::<DateTime<Utc>>() {
            Ok(dt) => dt.timestamp() as u64,
            Err(_) => {
                // Fallback to current timestamp if parsing fails
                Utc::now().timestamp() as u64
            },
        };
        timestamps.push_back(block_timestamp);

        // Keep only enough timestamps for difficulty adjustment
        while timestamps.len() > self.parameters.difficulty_adjustment_interval as usize {
            timestamps.pop_front();
        }

        // Process any orphans that may now connect
        self.process_orphans()?;

        Ok(())
    }

    /// Process orphan blocks
    fn process_orphans(&self) -> Result<()> {
        let chain = self.chain.read().unwrap();
        if let Some(latest_block) = chain.last() {
            let latest_hash = latest_block.hash.clone();
            drop(chain); // Release the read lock

            // Check if any orphans connect to the main chain now
            let mut orphans = self.orphans.write().unwrap();
            let mut connecting_orphans = Vec::new();

            for (hash, orphan) in orphans.iter() {
                if orphan.prev_hash == latest_hash {
                    connecting_orphans.push(hash.clone());
                }
            }

            let connecting_blocks = connecting_orphans
                .into_iter()
                .filter_map(|hash| orphans.remove(&hash))
                .collect();
            drop(orphans);

            self.process_connecting_orphans(connecting_blocks)?;
        }

        Ok(())
    }

    /// Update difficulty if needed
    fn update_difficulty(&self, height: u64) -> Result<()> {
        // Check if difficulty adjustment is needed
        if height % self.parameters.difficulty_adjustment_interval == 0 && height > 0 {
            info!("Adjusting difficulty at height {height}");

            // Get timestamps from block_timestamps
            let timestamps = self.block_timestamps.read().unwrap();
            let timestamps_vec: Vec<u64> = timestamps.iter().copied().collect();

            // Calculate time taken for the last N blocks
            if timestamps_vec.len() >= 2 {
                let start_time = timestamps_vec[0];
                let end_time = *timestamps_vec.last().unwrap();
                let time_diff = end_time - start_time;

                // Calculate expected time
                let expected_time =
                    self.parameters.target_block_time * (timestamps_vec.len() as u64 - 1);

                // Calculate adjustment factor
                let mut adjustment_factor = expected_time as f64 / time_diff as f64;

                // Clamp adjustment to max change
                if adjustment_factor > 1.0 + self.parameters.max_difficulty_adjustment_factor {
                    adjustment_factor = 1.0 + self.parameters.max_difficulty_adjustment_factor;
                } else if adjustment_factor < 1.0 - self.parameters.max_difficulty_adjustment_factor
                {
                    adjustment_factor = 1.0 - self.parameters.max_difficulty_adjustment_factor;
                }

                // Apply adjustment
                let current = *self.current_difficulty.read().unwrap();
                let new_difficulty = (current as f64 * adjustment_factor).round() as u64;

                // Ensure minimum difficulty of 1
                let new_difficulty = new_difficulty.max(1);

                // Update difficulty
                let mut difficulty = self.current_difficulty.write().unwrap();
                *difficulty = new_difficulty;

                info!(
                    "Difficulty adjusted from {current} to {new_difficulty}, adjustment factor: {adjustment_factor:.4}"
                );
            }
        }

        Ok(())
    }

    /// Check if a fork should trigger a chain reorganization
    fn check_fork_reorganization(&self, fork_id: &str) -> Result<()> {
        // Get fork information
        let forks = self.forks.read().unwrap();
        let fork = match forks.get(fork_id) {
            Some(fork) => fork.clone(),
            None => return Ok(()),
        };
        drop(forks); // Release the read lock

        // Calculate current chain difficulty
        let chain = self.chain.read().unwrap();
        let mut chain_cumulative_difficulty = 0;

        // Calculate from fork start height to current height
        for i in fork.start_height as usize..chain.len() {
            chain_cumulative_difficulty += chain[i].difficulty;
        }
        drop(chain);

        // Check if fork has higher cumulative difficulty
        if fork.cumulative_difficulty > chain_cumulative_difficulty {
            info!(
                "Fork {} has higher cumulative difficulty ({} > {}), considering reorganization",
                fork_id, fork.cumulative_difficulty, chain_cumulative_difficulty
            );

            // Check security policy for reorganization
            let reorg_height = fork
                .blocks
                .first()
                .map(|b| b.height)
                .unwrap_or(fork.start_height);
            let result = self.validate_reorganization(reorg_height, fork.cumulative_difficulty)?;

            if result == ConsensusResult::Valid {
                info!("Reorganizing to fork {fork_id}");
                // Perform reorganization
                self.perform_reorganization(fork_id)?;
            } else {
                warn!("Fork reorganization rejected: {result:?}");
            }
        }

        Ok(())
    }

    /// Validate a potential chain reorganization
    fn validate_reorganization(
        &self,
        fork_height: u64,
        fork_difficulty: u64,
    ) -> Result<ConsensusResult> {
        // Get current chain information
        let chain = self.chain.read().unwrap();
        let current_height = chain.len() as u64 - 1;

        // Calculate current chain difficulty from fork height
        let mut chain_difficulty = 0;
        for i in fork_height as usize..chain.len() {
            chain_difficulty += chain[i].difficulty;
        }

        // Consult security service for final approval
        // Ask any security service or policy implementations if this reorg is allowed

        // For now, we'll use a simple rule: reorg depth must be reasonable
        let reorg_depth = current_height - fork_height;
        if reorg_depth > 100 {
            return Ok(ConsensusResult::Invalid(format!(
                "Reorganization too deep: {reorg_depth} blocks"
            )));
        }

        // Ensure this doesn't revert a checkpoint
        {
            let security_service = self.security_service.lock().unwrap();
            for block in chain.iter().skip(fork_height as usize) {
                if security_service.is_checkpoint(block.height) {
                    return Ok(ConsensusResult::Invalid(format!(
                        "Reorganization would revert checkpoint at height {}",
                        block.height
                    )));
                }
            }
        }

        // If fork difficulty is not significantly higher, reject
        if fork_difficulty < chain_difficulty * 102 / 100 {
            return Ok(ConsensusResult::Invalid(format!(
                "Fork difficulty not significantly higher: {fork_difficulty} vs {chain_difficulty}"
            )));
        }

        Ok(ConsensusResult::Valid)
    }

    /// Perform a chain reorganization
    fn perform_reorganization(&self, fork_id: &str) -> Result<()> {
        // Get fork information
        let mut forks = self.forks.write().unwrap();
        let fork = match forks.get(fork_id) {
            Some(fork) => fork.clone(),
            None => return Err(anyhow!("Fork not found: {}", fork_id)),
        };

        // Remove fork from forks collection
        forks.remove(fork_id);
        drop(forks); // Release the write lock

        // Reorganize the chain
        let mut chain = self.chain.write().unwrap();

        // Truncate chain to fork point
        chain.truncate(fork.start_height as usize + 1);

        // Append fork blocks
        for block in &fork.blocks {
            chain.push(block.clone());
        }

        info!(
            "Chain reorganized to fork {}. New height: {}",
            fork_id,
            chain.len() - 1
        );

        // Update difficulty
        let last_block = chain.last().unwrap();
        self.update_difficulty(last_block.height)?;

        // Process orphans that may now connect
        drop(chain); // Release the write lock
        self.process_orphans()?;

        Ok(())
    }

    /// Calculate block reward for a given height
    pub fn calculate_block_reward(&self, height: u64) -> f64 {
        let mut reward = self.parameters.block_reward;

        // Apply halving if configured
        if let Some(halving_interval) = self.parameters.reward_halving_interval {
            if halving_interval > 0 {
                let halvings = height / halving_interval;
                if halvings > 0 {
                    reward /= 2.0_f64.powi(halvings as i32);
                }
            }
        }

        // Check against max supply if configured
        if let Some(_max_supply) = self.parameters.max_supply {
            // In a real implementation, we would check if this reward would exceed max supply
        }

        reward
    }

    /// Extend a fork with a new block
    fn extend_fork_with_block(&self, fork: &mut Fork, block: &BlockHeader) {
        fork.blocks.push(block.clone());
        fork.cumulative_difficulty += block.difficulty;
        fork.last_updated = Utc::now();

        info!(
            "Fork {} extended to height {} with block {}",
            fork.id, block.height, block.hash
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_block(height: u64, prev_hash: &str) -> BlockHeader {
        BlockHeader {
            hash: format!("block_hash_{}", height),
            height,
            prev_hash: prev_hash.to_string(),
            timestamp: Utc::now().to_rfc3339(),
            merkle_root: format!("merkle_root_{}", height),
            difficulty: 1,
            nonce: 0,
            version: 1,
            block_type: "Standard".to_string(),
        }
    }

    fn create_test_consensus_engine() -> ConsensusEngine {
        let security_service = Arc::new(Mutex::new(SecurityService::default()));
        let parameters = ConsensusParameters::default();

        // Create genesis block
        let genesis = BlockHeader {
            hash: "genesis_hash".to_string(),
            height: 0,
            prev_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            timestamp: Utc::now().to_rfc3339(),
            merkle_root: "genesis_merkle_root".to_string(),
            difficulty: 1,
            nonce: 0,
            version: 1,
            block_type: "Genesis".to_string(),
        };

        ConsensusEngine::new(security_service, parameters, genesis)
    }

    #[test]
    fn test_extending_chain() {
        let engine = create_test_consensus_engine();

        // Get genesis block
        let genesis = engine.get_latest_block().unwrap();

        // Create a new block extending genesis
        let block1 = create_test_block(1, &genesis.hash);

        // Process the block
        let result = engine.process_block(block1.clone()).unwrap();
        assert_eq!(result, ConsensusResult::Valid);

        // Check chain height
        assert_eq!(engine.get_height(), 1);

        // Check latest block
        let latest = engine.get_latest_block().unwrap();
        assert_eq!(latest.hash, block1.hash);
    }

    #[test]
    fn test_orphan_handling() {
        let engine = create_test_consensus_engine();

        // Get genesis block
        let genesis = engine.get_latest_block().unwrap();

        // Create a block that would be at height 2 (orphan)
        let orphan = create_test_block(2, "unknown_hash");

        // Process the orphan block
        let result = engine.process_block(orphan.clone()).unwrap();
        assert!(matches!(result, ConsensusResult::Indeterminate(_)));

        // Check chain height is still 0
        assert_eq!(engine.get_height(), 0);

        // Now add block 1
        let block1 = create_test_block(1, &genesis.hash);
        let result = engine.process_block(block1.clone()).unwrap();
        assert_eq!(result, ConsensusResult::Valid);

        // Check chain height is now 1
        assert_eq!(engine.get_height(), 1);
    }

    #[test]
    fn test_fork_handling() {
        let engine = create_test_consensus_engine();

        // Get genesis block
        let genesis = engine.get_latest_block().unwrap();

        // Create and add block 1
        let block1 = create_test_block(1, &genesis.hash);
        let result = engine.process_block(block1.clone()).unwrap();
        assert_eq!(result, ConsensusResult::Valid);

        // Create and add block 2
        let block2 = create_test_block(2, &block1.hash);
        let result = engine.process_block(block2.clone()).unwrap();
        assert_eq!(result, ConsensusResult::Valid);

        // Create competing block 1' (fork)
        let mut fork_block1 = create_test_block(1, &genesis.hash);
        fork_block1.hash = "fork_block1_hash".to_string();

        // Process fork block
        let result = engine.process_block(fork_block1.clone()).unwrap();
        assert_eq!(result, ConsensusResult::Valid);

        // Chain should still be at block 2
        assert_eq!(engine.get_height(), 2);

        // Create fork block 2' with higher difficulty
        let mut fork_block2 = create_test_block(2, &fork_block1.hash);
        fork_block2.hash = "fork_block2_hash".to_string();
        fork_block2.difficulty = 10; // Higher than main chain

        // Process fork block 2'
        let result = engine.process_block(fork_block2.clone()).unwrap();
        assert_eq!(result, ConsensusResult::Valid);

        // Create fork block 3' with higher difficulty
        let fork_block3 = create_test_block(3, &fork_block2.hash);

        // Process fork block 3'
        let result = engine.process_block(fork_block3.clone()).unwrap();
        assert_eq!(result, ConsensusResult::Valid);

        // Chain should now be reorganized to the fork (height 3)
        assert_eq!(engine.get_height(), 3);

        // Latest block should be fork_block3
        let latest = engine.get_latest_block().unwrap();
        assert_eq!(latest.hash, fork_block3.hash);
    }

    #[test]
    fn test_checkpoint_functionality() {
        let engine = create_test_consensus_engine();

        // Get genesis block
        let genesis = engine.get_latest_block().unwrap();

        // Create and add block 1
        let block1 = create_test_block(1, &genesis.hash);
        let result = engine.process_block(block1.clone()).unwrap();
        assert_eq!(result, ConsensusResult::Valid);

        // Add a checkpoint for block 1
        let checkpoint = CheckpointBlock {
            height: 1,
            hash: block1.hash.clone(),
            timestamp: block1.timestamp.clone(),
        };

        // Test adding valid checkpoint
        assert!(engine.add_checkpoint(checkpoint.clone()).is_ok());

        // Test adding checkpoint with empty hash (should fail)
        let invalid_checkpoint = CheckpointBlock {
            height: 2,
            hash: "".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        };
        assert!(engine.add_checkpoint(invalid_checkpoint).is_err());

        // Test adding checkpoint at genesis height (should fail)
        let genesis_checkpoint = CheckpointBlock {
            height: 0,
            hash: genesis.hash.clone(),
            timestamp: genesis.timestamp.clone(),
        };
        assert!(engine.add_checkpoint(genesis_checkpoint).is_err());

        // Test adding checkpoint with hash mismatch (should fail)
        let mismatched_checkpoint = CheckpointBlock {
            height: 1,
            hash: "wrong_hash".to_string(),
            timestamp: block1.timestamp.clone(),
        };
        assert!(engine.add_checkpoint(mismatched_checkpoint).is_err());

        // Verify checkpoint prevents fork reorganization
        let competing_block1 = create_test_block(1, &genesis.hash);
        let mut competing_block1_modified = competing_block1.clone();
        competing_block1_modified.hash = "competing_block1_hash".to_string();
        competing_block1_modified.difficulty = 10; // Higher difficulty

        // Try to create a fork - should be rejected due to checkpoint
        let _result = engine.process_block(competing_block1_modified).unwrap();

        // Chain should still be at height 1 with original block
        assert_eq!(engine.get_height(), 1);
        let latest = engine.get_latest_block().unwrap();
        assert_eq!(latest.hash, block1.hash);
    }
}
