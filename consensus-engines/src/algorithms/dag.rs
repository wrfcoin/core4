//! DAG-based consensus algorithm implementation
//!
//! Adapted from `blockchain-core/src/consensus/dag_consensus.rs`.
//! Implements a directed acyclic graph (DAG) consensus protocol inspired by
//! PHANTOM/GHOSTDAG. Unlike traditional linear blockchains, multiple blocks
//! can be produced in parallel for the same slot. The DAG structure allows
//! higher throughput while maintaining consensus through a scoring algorithm
//! that identifies the honest "blue set" of blocks.
//!
//! The core DAG types (`DagBlock`, `BlockDag`) are self-contained. A
//! `DagConsensusEngine` wrapper implements `ConsensusEngine` to bridge
//! between the DAG world and the shared-protocol linear `Block` type.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::crypto::hash::hash_bytes;

use crate::traits::ConsensusEngine;
use crate::types::{ConsensusConfig, ConsensusError, ConsensusResult};

// ---------------------------------------------------------------------------
// DAG-internal types
// ---------------------------------------------------------------------------

/// Hash type for DAG blocks (hex-encoded SHA-256)
pub type BlockHash = String;

/// Errors that can occur in DAG consensus operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum DagError {
    #[error("Block already exists: {0}")]
    BlockAlreadyExists(BlockHash),

    #[error("Parent block not found: {0}")]
    ParentNotFound(BlockHash),

    #[error("No parents specified")]
    NoParents,

    #[error("Block not found: {0}")]
    BlockNotFound(BlockHash),

    #[error("Cycle detected: adding block would create a cycle")]
    CycleDetected,

    #[error("DAG is empty")]
    EmptyDag,

    /// Returned when an untrusted block carries more parent references than
    /// `DagConfig::max_parents` allows. Without this guard, a single malicious
    /// peer block with thousands of parents triggers O(P) work in `add_block`
    /// and O(B²·P) GHOSTDAG re-scoring on every subsequent insertion (DoS).
    #[error("Block has {got} parents but maximum is {max}")]
    TooManyParents { got: usize, max: usize },
}

/// A block in the DAG with multiple parent references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagBlock {
    /// Block hash (unique identifier)
    pub hash: BlockHash,
    /// References to parent blocks (multiple parents allowed)
    pub parents: Vec<BlockHash>,
    /// Block height (max parent height + 1)
    pub height: u64,
    /// Slot number when this block was produced
    pub slot: u64,
    /// Producer/validator address
    pub producer: String,
    /// Timestamp
    pub timestamp: u64,
    /// Serialized transaction data (opaque to DAG layer)
    pub payload: Vec<u8>,
    /// Number of transactions in this block
    pub transaction_count: u32,
    /// GHOSTDAG score (computed after insertion)
    pub score: u64,
    /// Whether this block is in the blue set (honest)
    pub is_blue: bool,
}

impl DagBlock {
    /// Create a new DAG block.
    pub fn new(
        parents: Vec<BlockHash>,
        slot: u64,
        producer: String,
        timestamp: u64,
        payload: Vec<u8>,
        transaction_count: u32,
    ) -> Self {
        let hash = Self::compute_hash(&parents, slot, &producer, timestamp, &payload);
        Self {
            hash,
            parents,
            height: 0, // computed during insertion
            slot,
            producer,
            timestamp,
            payload,
            transaction_count,
            score: 0,
            is_blue: false,
        }
    }

    /// Compute the block hash from its contents.
    fn compute_hash(
        parents: &[BlockHash],
        slot: u64,
        producer: &str,
        timestamp: u64,
        payload: &[u8],
    ) -> BlockHash {
        let mut hasher = Sha256::new();
        hasher.update(b"wrfcoin-dag-block-v1|");
        for parent in parents {
            hasher.update(parent.as_bytes());
            hasher.update(b"|");
        }
        hasher.update(slot.to_be_bytes());
        hasher.update(producer.as_bytes());
        hasher.update(timestamp.to_be_bytes());
        hasher.update(payload);
        hex::encode(hasher.finalize())
    }
}

/// Configuration for the DAG consensus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagConfig {
    /// Parameter k: maximum number of blocks in the anticone of a blue block
    /// that can also be blue. Higher k = more tolerance for parallel blocks.
    pub k: usize,
    /// Number of confirmations required for finality
    pub finality_depth: u64,
    /// Maximum number of parents per block
    pub max_parents: usize,
}

impl Default for DagConfig {
    fn default() -> Self {
        Self {
            k: 18,
            finality_depth: 10,
            max_parents: 8,
        }
    }
}

/// Directed Acyclic Graph of blocks.
///
/// Supports parallel block production and uses GHOSTDAG-style scoring
/// to determine the canonical ordering.
#[derive(Debug, Clone)]
pub struct BlockDag {
    /// All blocks indexed by hash
    blocks: FxHashMap<BlockHash, DagBlock>,
    /// Children index: parent_hash -> set of child hashes
    children: FxHashMap<BlockHash, FxHashSet<BlockHash>>,
    /// Tips: blocks with no children (leaf nodes)
    tips: FxHashSet<BlockHash>,
    /// Genesis block hash
    genesis: Option<BlockHash>,
    /// Blue set: blocks considered honest by GHOSTDAG
    blue_set: FxHashSet<BlockHash>,
    /// DAG configuration
    config: DagConfig,
    /// Cached topological order (invalidated on insertion)
    topo_cache: Option<Vec<BlockHash>>,
}

impl BlockDag {
    /// Create a new empty DAG with default configuration.
    pub fn new() -> Self {
        Self {
            blocks: FxHashMap::default(),
            children: FxHashMap::default(),
            tips: FxHashSet::default(),
            genesis: None,
            blue_set: FxHashSet::default(),
            config: DagConfig::default(),
            topo_cache: None,
        }
    }

    /// Create a new DAG with custom configuration.
    pub fn with_config(config: DagConfig) -> Self {
        Self {
            blocks: FxHashMap::default(),
            children: FxHashMap::default(),
            tips: FxHashSet::default(),
            genesis: None,
            blue_set: FxHashSet::default(),
            config,
            topo_cache: None,
        }
    }

    /// Add the genesis block (a block with no parents).
    pub fn add_genesis(
        &mut self,
        producer: String,
        timestamp: u64,
        payload: Vec<u8>,
    ) -> Result<BlockHash, DagError> {
        let mut block = DagBlock::new(vec![], 0, producer, timestamp, payload, 0);
        block.height = 0;
        block.is_blue = true;
        block.score = 1;

        let hash = block.hash.clone();
        self.blocks.insert(hash.clone(), block);
        self.tips.insert(hash.clone());
        self.genesis = Some(hash.clone());
        self.blue_set.insert(hash.clone());
        self.topo_cache = None;

        Ok(hash)
    }

    /// Add a block to the DAG with parent validation.
    pub fn add_block(&mut self, mut block: DagBlock) -> Result<BlockHash, DagError> {
        if block.parents.is_empty() && self.genesis.is_some() {
            return Err(DagError::NoParents);
        }

        // DoS guard: reject blocks whose parent count exceeds the configured
        // maximum BEFORE touching any state or allocating the past-set. A
        // malicious peer can craft a block with thousands of parents; without
        // this check each such block triggers O(P) work here and O(B²·P) work
        // in update_ghostdag_scoring on every subsequent insertion.
        if block.parents.len() > self.config.max_parents {
            return Err(DagError::TooManyParents {
                got: block.parents.len(),
                max: self.config.max_parents,
            });
        }

        if self.blocks.contains_key(&block.hash) {
            return Err(DagError::BlockAlreadyExists(block.hash.clone()));
        }

        let mut max_parent_height: u64 = 0;
        for parent_hash in &block.parents {
            let parent = self
                .blocks
                .get(parent_hash)
                .ok_or_else(|| DagError::ParentNotFound(parent_hash.clone()))?;
            max_parent_height = max_parent_height.max(parent.height);
        }

        block.height = max_parent_height + 1;

        let hash = block.hash.clone();

        for parent_hash in &block.parents {
            self.children
                .entry(parent_hash.clone())
                .or_default()
                .insert(hash.clone());
            self.tips.remove(parent_hash);
        }

        self.tips.insert(hash.clone());
        self.blocks.insert(hash.clone(), block);
        self.topo_cache = None;

        self.update_ghostdag_scoring(&hash);

        Ok(hash)
    }

    /// Get a block by its hash.
    pub fn get_block(&self, hash: &str) -> Option<&DagBlock> {
        self.blocks.get(hash)
    }

    /// Get the current tip blocks.
    pub fn get_tips(&self) -> &FxHashSet<BlockHash> {
        &self.tips
    }

    /// Get the genesis block hash.
    pub fn genesis(&self) -> Option<&BlockHash> {
        self.genesis.as_ref()
    }

    /// Get the total number of blocks in the DAG.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Check if the DAG is empty.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Get the blue set (honest blocks).
    pub fn blue_set(&self) -> &FxHashSet<BlockHash> {
        &self.blue_set
    }

    /// Get the children of a block.
    pub fn get_children(&self, hash: &str) -> Option<&FxHashSet<BlockHash>> {
        self.children.get(hash)
    }

    /// Get a deterministic topological ordering of all blocks.
    pub fn get_topological_order(&mut self) -> Vec<DagBlock> {
        if let Some(ref cached) = self.topo_cache {
            return cached
                .iter()
                .filter_map(|h| self.blocks.get(h).cloned())
                .collect();
        }

        let order = self.compute_topological_order();
        self.topo_cache = Some(order.iter().map(|b| b.hash.clone()).collect());
        order
    }

    /// Compute topological order using Kahn's algorithm.
    #[allow(clippy::excessive_nesting)]
    fn compute_topological_order(&self) -> Vec<DagBlock> {
        if self.blocks.is_empty() {
            return vec![];
        }

        let mut in_degree: FxHashMap<&str, usize> = FxHashMap::default();
        for hash in self.blocks.keys() {
            in_degree.entry(hash.as_str()).or_insert(0);
        }
        for block in self.blocks.values() {
            *in_degree.entry(block.hash.as_str()).or_insert(0) = block.parents.len();
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&hash, _)| hash)
            .collect();

        let mut queue_vec: Vec<&str> = queue.drain(..).collect();
        queue_vec.sort_by(|a, b| {
            let ha = self.blocks.get(*a).map(|blk| blk.height);
            let hb = self.blocks.get(*b).map(|blk| blk.height);
            ha.cmp(&hb).then_with(|| a.cmp(b))
        });
        queue = queue_vec.into_iter().collect();

        let mut result = Vec::new();

        while let Some(hash) = queue.pop_front() {
            if let Some(block) = self.blocks.get(hash) {
                result.push(block.clone());

                if let Some(children) = self.children.get(hash) {
                    // Filter out dangling references to blocks not in self.blocks
                    // (can occur from malicious peers or pruning races — issue #957).
                    let mut child_list: Vec<&str> = children
                        .iter()
                        .filter(|s| self.blocks.contains_key(s.as_str()))
                        .map(|s| s.as_str())
                        .collect();

                    for child_hash in &child_list {
                        if let Some(deg) = in_degree.get_mut(child_hash) {
                            *deg = deg.saturating_sub(1);
                        }
                    }

                    child_list.sort_by(|a, b| {
                        let ha = self.blocks.get(*a).map(|blk| blk.height);
                        let hb = self.blocks.get(*b).map(|blk| blk.height);
                        ha.cmp(&hb).then_with(|| a.cmp(b))
                    });

                    for child_hash in child_list {
                        if in_degree.get(child_hash).copied().unwrap_or(0) == 0 {
                            queue.push_back(child_hash);
                        }
                    }
                }
            }
        }

        result
    }

    /// Calculate the GHOSTDAG score for a block.
    pub fn calculate_score(&self, block: &DagBlock) -> u64 {
        let past = self.get_past_set(&block.hash);
        past.iter().filter(|h| self.blue_set.contains(*h)).count() as u64
            + if self.blue_set.contains(&block.hash) {
                1
            } else {
                0
            }
    }

    /// Find the blue set using GHOSTDAG algorithm.
    pub fn find_blue_set(&self) -> FxHashSet<BlockHash> {
        let topo = self.compute_topological_order();
        let past_sets = self.compute_past_sets_incremental(&topo);
        self.find_blue_set_with_past_sets(&topo, &past_sets)
    }

    /// Compute past sets for all blocks incrementally via topological propagation.
    /// Each block's past = union of all parents' past sets + parents themselves.
    /// Total work: O(sum of past set sizes) which is O(B^2) in the worst case,
    /// but avoids repeated BFS traversals.
    fn compute_past_sets_incremental(
        &self,
        topo: &[DagBlock],
    ) -> FxHashMap<BlockHash, FxHashSet<BlockHash>> {
        let mut past_sets: FxHashMap<BlockHash, FxHashSet<BlockHash>> = FxHashMap::default();
        for block in topo {
            let mut past = FxHashSet::default();
            for parent_hash in &block.parents {
                past.insert(parent_hash.clone());
                if let Some(parent_past) = past_sets.get(parent_hash) {
                    past.extend(parent_past.iter().cloned());
                }
            }
            past_sets.insert(block.hash.clone(), past);
        }
        past_sets
    }

    /// Find blue set using pre-computed past sets (avoids per-block BFS).
    /// Anticone membership is checked via past-set containment:
    /// X is in anticone(block) iff X not in past(block) AND block not in past(X).
    fn find_blue_set_with_past_sets(
        &self,
        topo: &[DagBlock],
        past_sets: &FxHashMap<BlockHash, FxHashSet<BlockHash>>,
    ) -> FxHashSet<BlockHash> {
        let mut blue = FxHashSet::default();

        if let Some(ref genesis) = self.genesis {
            blue.insert(genesis.clone());
        }

        for block in topo {
            if Some(&block.hash) == self.genesis.as_ref() {
                continue;
            }
            let block_past = match past_sets.get(&block.hash) {
                Some(p) => p,
                None => continue,
            };

            // Count blue blocks in the anticone of this block.
            let mut blue_in_anticone = 0usize;
            for blue_hash in &blue {
                if *blue_hash == block.hash {
                    continue;
                }
                // blue_hash is in past(block) → not in anticone
                if block_past.contains(blue_hash) {
                    continue;
                }
                // block is in past(blue_hash) → blue_hash is in future(block) → not in anticone
                if past_sets.get(blue_hash).is_some_and(|bp| bp.contains(&block.hash)) {
                    continue;
                }
                // blue_hash is in the anticone
                blue_in_anticone += 1;
            }

            if blue_in_anticone <= self.config.k {
                blue.insert(block.hash.clone());
            }
        }

        blue
    }

    /// Get the past set (all ancestors) of a block.
    #[allow(clippy::excessive_nesting)]
    fn get_past_set(&self, hash: &str) -> FxHashSet<BlockHash> {
        let mut past = FxHashSet::default();
        let mut stack = VecDeque::new();

        if let Some(block) = self.blocks.get(hash) {
            for parent in &block.parents {
                stack.push_back(parent.clone());
            }
        }

        while let Some(current) = stack.pop_front() {
            if past.insert(current.clone()) {
                if let Some(block) = self.blocks.get(&current) {
                    for parent in &block.parents {
                        if !past.contains(parent) {
                            stack.push_back(parent.clone());
                        }
                    }
                }
            }
        }

        past
    }

    /// Update GHOSTDAG scoring for a newly added block.
    ///
    /// Uses incremental past-set computation to avoid O(B^2+) repeated BFS (#704).
    fn update_ghostdag_scoring(&mut self, _hash: &BlockHash) {
        let topo = self.compute_topological_order();
        let past_sets = self.compute_past_sets_incremental(&topo);
        let new_blue_set = self.find_blue_set_with_past_sets(&topo, &past_sets);
        self.blue_set = new_blue_set;

        // Compute scores using cached past sets instead of per-block BFS.
        for (h, past) in &past_sets {
            let is_blue = self.blue_set.contains(h);
            let score = past.iter().filter(|p| self.blue_set.contains(*p)).count() as u64
                + if is_blue { 1 } else { 0 };

            if let Some(block) = self.blocks.get_mut(h) {
                block.is_blue = is_blue;
                block.score = score;
            }
        }
    }

    /// Check if a block is finalized based on DAG depth.
    pub fn is_finalized(&self, hash: &str) -> bool {
        let block = match self.blocks.get(hash) {
            Some(b) => b,
            None => return false,
        };

        let max_tip_height = self
            .tips
            .iter()
            .filter_map(|h| self.blocks.get(h))
            .map(|b| b.height)
            .max()
            .unwrap_or(0);

        max_tip_height >= block.height + self.config.finality_depth
    }

    /// Get all finalized blocks.
    pub fn get_finalized_blocks(&self) -> Vec<&DagBlock> {
        let max_tip_height = self
            .tips
            .iter()
            .filter_map(|h| self.blocks.get(h))
            .map(|b| b.height)
            .max()
            .unwrap_or(0);

        self.blocks
            .values()
            .filter(|b| max_tip_height >= b.height + self.config.finality_depth)
            .collect()
    }

    /// Get the selected chain tip (highest-scoring blue tip).
    pub fn selected_tip(&self) -> Option<&DagBlock> {
        self.tips
            .iter()
            .filter_map(|h| self.blocks.get(h))
            .filter(|b| b.is_blue)
            .max_by_key(|b| b.score)
    }

    /// Get the DAG configuration.
    pub fn config(&self) -> &DagConfig {
        &self.config
    }
}

impl Default for BlockDag {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// DagConsensusEngine - ConsensusEngine wrapper
// ---------------------------------------------------------------------------

/// Wrapper around `BlockDag` that implements the `ConsensusEngine` trait.
///
/// Bridges between the DAG world (parallel blocks, tips) and the
/// shared-protocol linear `Block` type.
#[derive(Debug)]
pub struct DagConsensusEngine {
    /// Internal DAG
    dag: std::sync::Arc<tokio::sync::RwLock<BlockDag>>,
    /// Target block time
    target_block_time: u64,
    /// Algorithm configuration
    config: ConsensusConfig,
}

impl DagConsensusEngine {
    /// Create a new DAG consensus engine with default DAG config.
    pub fn new(target_block_time: u64) -> Self {
        Self {
            dag: std::sync::Arc::new(tokio::sync::RwLock::new(BlockDag::new())),
            target_block_time,
            config: ConsensusConfig::default(),
        }
    }

    /// Create from an existing BlockDag.
    pub fn with_dag(dag: BlockDag, target_block_time: u64) -> Self {
        Self {
            dag: std::sync::Arc::new(tokio::sync::RwLock::new(dag)),
            target_block_time,
            config: ConsensusConfig::default(),
        }
    }

    /// Get a reference to the internal DAG.
    pub fn dag(&self) -> &std::sync::Arc<tokio::sync::RwLock<BlockDag>> {
        &self.dag
    }
}

#[async_trait]
impl ConsensusEngine for DagConsensusEngine {
    async fn validate_block(
        &self,
        block: &Block,
        _chain: &Blockchain,
    ) -> ConsensusResult<bool> {
        // Check consensus type
        if let Some(ct) = block.metadata.get("consensus_type") {
            if ct != "DAG" {
                return Ok(false);
            }
        }

        // For DAG blocks wrapped in linear Block, check the dag_hash metadata
        if let Some(dag_hash) = block.metadata.get("dag_hash") {
            let dag = self.dag.read().await;
            if let Some(dag_block) = dag.get_block(dag_hash) {
                return Ok(dag_block.is_blue);
            }
            return Ok(false);
        }

        // Basic validity check
        Ok(block.hash.is_some())
    }

    async fn create_block(
        &self,
        transactions: Vec<Transaction>,
        previous_hash: &str,
        producer: &str,
        chain: &Blockchain,
    ) -> ConsensusResult<Block> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Serialize transactions as the DAG block payload
        let payload =
            serde_json::to_vec(&transactions).unwrap_or_default();
        let tx_count = transactions.len() as u32;

        // Get current tips as parents for the DAG block
        let dag_hash = {
            let mut dag = self.dag.write().await;

            // Ensure genesis exists
            if dag.genesis().is_none() {
                dag.add_genesis(producer.to_string(), timestamp, vec![])
                    .map_err(|e| {
                        ConsensusError::CreationFailed(format!(
                            "Failed to create DAG genesis: {e}"
                        ))
                    })?;
            }

            let parents: Vec<BlockHash> = dag.get_tips().iter().cloned().collect();
            let slot = chain.height + 1;

            let dag_block =
                DagBlock::new(parents, slot, producer.to_string(), timestamp, payload, tx_count);

            dag.add_block(dag_block).map_err(|e| {
                ConsensusError::CreationFailed(format!("Failed to add DAG block: {e}"))
            })?
        };

        // Wrap in a shared-protocol Block
        let prev = hash_bytes(previous_hash.as_bytes());
        let mut block = Block::new(3, prev, transactions, 1, chain.height + 1);

        block
            .metadata
            .insert("validator".into(), producer.into());
        block
            .metadata
            .insert("consensus_type".into(), "DAG".into());
        block
            .metadata
            .insert("dag_hash".into(), dag_hash);

        let hash = block.calculate_hash();
        block.set_hash(hash);

        Ok(block)
    }

    async fn get_difficulty(&self) -> u64 {
        1
    }

    fn algorithm_name(&self) -> &'static str {
        "DAG"
    }

    async fn configure(&mut self, config: &ConsensusConfig) -> ConsensusResult<()> {
        self.config = config.clone();
        self.target_block_time = config.target_block_time;
        Ok(())
    }

    fn get_metrics(&self) -> FxHashMap<String, serde_json::Value> {
        let mut metrics = FxHashMap::default();
        metrics.insert("algorithm".to_string(), "DAG".into());
        metrics.insert(
            "target_block_time".to_string(),
            self.target_block_time.into(),
        );
        metrics
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(parents: Vec<BlockHash>, slot: u64, producer: &str) -> DagBlock {
        DagBlock::new(
            parents,
            slot,
            producer.to_string(),
            1000 + slot,
            format!("payload-{}-{}", producer, slot).into_bytes(),
            5,
        )
    }

    #[test]
    fn test_genesis_creation() {
        let mut dag = BlockDag::new();
        let hash = dag
            .add_genesis("validator-0".to_string(), 0, vec![])
            .unwrap();

        assert_eq!(dag.len(), 1);
        assert!(dag.genesis().is_some());
        assert_eq!(*dag.genesis().unwrap(), hash);

        let block = dag.get_block(&hash).unwrap();
        assert_eq!(block.height, 0);
        assert!(block.is_blue);
    }

    #[test]
    fn test_add_block_single_parent() {
        let mut dag = BlockDag::new();
        let genesis = dag.add_genesis("val-0".to_string(), 0, vec![]).unwrap();

        let block = make_block(vec![genesis.clone()], 1, "val-1");
        let hash = dag.add_block(block).unwrap();

        assert_eq!(dag.len(), 2);
        let b = dag.get_block(&hash).unwrap();
        assert_eq!(b.height, 1);
    }

    #[test]
    fn test_tips_tracking() {
        let mut dag = BlockDag::new();
        let genesis = dag.add_genesis("val-0".to_string(), 0, vec![]).unwrap();

        assert_eq!(dag.get_tips().len(), 1);

        let b1 = make_block(vec![genesis.clone()], 1, "val-1");
        let h1 = dag.add_block(b1).unwrap();

        let b2 = make_block(vec![genesis.clone()], 1, "val-2");
        let h2 = dag.add_block(b2).unwrap();

        assert_eq!(dag.get_tips().len(), 2);
        assert!(!dag.get_tips().contains(&genesis));

        let b3 = make_block(vec![h1, h2], 2, "val-3");
        let h3 = dag.add_block(b3).unwrap();

        assert_eq!(dag.get_tips().len(), 1);
        assert!(dag.get_tips().contains(&h3));
    }

    #[test]
    fn test_finality() {
        let config = DagConfig {
            k: 18,
            finality_depth: 3,
            max_parents: 8,
        };
        let mut dag = BlockDag::with_config(config);
        let genesis = dag.add_genesis("val-0".to_string(), 0, vec![]).unwrap();

        let mut prev = genesis.clone();
        for i in 1..=5 {
            let b = make_block(vec![prev.clone()], i, &format!("val-{}", i));
            prev = dag.add_block(b).unwrap();
        }

        assert!(dag.is_finalized(&genesis));
        assert!(!dag.is_finalized(&prev));
    }

    /// Regression test for issue #704: 500+ block DAG scoring must complete
    /// in bounded time with incremental past-set computation.
    #[test]
    fn dag_500_blocks_scoring_bounded() {
        let config = DagConfig {
            k: 18,
            finality_depth: 10,
            max_parents: 4,
        };
        let mut dag = BlockDag::with_config(config);
        let genesis = dag
            .add_genesis("val-0".to_string(), 0, vec![])
            .unwrap();

        // Build a DAG with 500 blocks. Each block references 1-3 recent parents
        // to create realistic DAG width.
        let mut recent_tips = vec![genesis.clone()];
        for i in 1..=500u64 {
            // Pick parents from the most recent tips (up to 3).
            let num_parents = (1 + (i % 3) as usize).min(recent_tips.len());
            let parents: Vec<BlockHash> = recent_tips
                .iter()
                .rev()
                .take(num_parents)
                .cloned()
                .collect();
            let b = make_block(parents, i, &format!("val-{}", i % 10));
            let hash = dag.add_block(b).unwrap();
            recent_tips.push(hash);
            // Keep last 10 tips to control DAG width.
            if recent_tips.len() > 10 {
                recent_tips.remove(0);
            }
        }

        assert!(dag.len() > 500);
        assert!(!dag.blue_set().is_empty());

        // Time a fresh scoring pass on the 500+ block DAG.
        let start = std::time::Instant::now();
        let topo = dag.compute_topological_order();
        let past_sets = dag.compute_past_sets_incremental(&topo);
        let blue = dag.find_blue_set_with_past_sets(&topo, &past_sets);
        let elapsed = start.elapsed();

        assert!(!blue.is_empty(), "blue set must not be empty");
        assert!(blue.contains(&genesis), "genesis must be blue");
        assert!(
            elapsed.as_millis() < 5000,
            "500-block DAG scoring took {elapsed:?}, expected <5s"
        );
    }

    // ── DoS guard: max_parents enforcement ──────────────────────────────────
    //
    // DagConfig::max_parents caps the number of parent references per block.
    // Without enforcement, a malicious peer could submit a block with thousands
    // of parents and trigger O(P) work in add_block + O(B²·P) GHOSTDAG
    // re-scoring on every subsequent insertion.

    /// `add_block` must reject a block whose parent count exceeds
    /// `DagConfig::max_parents` before modifying any internal state.
    #[test]
    fn dag_add_block_rejects_too_many_parents() {
        let config = DagConfig {
            k: 18,
            finality_depth: 10,
            max_parents: 3, // tighter bound for the test
        };
        let mut dag = BlockDag::with_config(config);
        let genesis = dag
            .add_genesis("val-0".to_string(), 0, vec![])
            .unwrap();

        // Build 4 legitimate blocks so we have real parent hashes available.
        let mut siblings = Vec::new();
        for i in 1..=4u64 {
            let b = make_block(vec![genesis.clone()], i, &format!("val-{i}"));
            let h = dag.add_block(b).unwrap();
            siblings.push(h);
        }

        // Craft a block referencing 4 parents — one more than max_parents=3.
        // add_block must reject it before touching any DAG state.
        let oversized = make_block(siblings, 5, "attacker");
        let err = dag.add_block(oversized).unwrap_err();
        assert!(
            matches!(err, DagError::TooManyParents { got: 4, max: 3 }),
            "expected TooManyParents(got=4, max=3), got {err:?}"
        );
    }

    /// A block with exactly `max_parents` references must be accepted.
    #[test]
    fn dag_add_block_accepts_exactly_max_parents() {
        let config = DagConfig {
            k: 18,
            finality_depth: 10,
            max_parents: 3,
        };
        let mut dag = BlockDag::with_config(config);
        let genesis = dag
            .add_genesis("val-0".to_string(), 0, vec![])
            .unwrap();

        let mut siblings = Vec::new();
        for i in 1..=3u64 {
            let b = make_block(vec![genesis.clone()], i, &format!("val-{i}"));
            let h = dag.add_block(b).unwrap();
            siblings.push(h);
        }

        // Exactly 3 parents — must succeed.
        let at_limit = make_block(siblings, 4, "val-merge");
        assert!(
            dag.add_block(at_limit).is_ok(),
            "block with exactly max_parents references must be accepted"
        );
    }

    /// Regression test for issue #957: topological_order must not panic when
    /// the children index contains dangling references to blocks that are
    /// missing from the blocks map (e.g. pruned or from a malicious peer).
    #[test]
    fn topological_order_with_dangling_child_references() {
        let mut dag = BlockDag::new();
        let genesis = dag.add_genesis("val-0".to_string(), 0, vec![]).unwrap();

        let b1 = make_block(vec![genesis.clone()], 1, "val-1");
        let h1 = dag.add_block(b1).unwrap();

        let b2 = make_block(vec![h1.clone()], 2, "val-2");
        let h2 = dag.add_block(b2).unwrap();

        // Inject dangling child references that point to blocks not in the
        // blocks map — simulates a pruning race or malicious peer data.
        let phantom_a = "phantom_block_aaa".to_string();
        let phantom_b = "phantom_block_bbb".to_string();
        dag.children
            .entry(h1.clone())
            .or_default()
            .insert(phantom_a.clone());
        dag.children
            .entry(h2.clone())
            .or_default()
            .insert(phantom_b.clone());

        // Must not panic.
        let order = dag.get_topological_order();

        // Only real blocks should appear in the result.
        assert_eq!(order.len(), 3);
        let hashes: Vec<&str> = order.iter().map(|b| b.hash.as_str()).collect();
        assert!(hashes.contains(&genesis.as_str()));
        assert!(hashes.contains(&h1.as_str()));
        assert!(hashes.contains(&h2.as_str()));
        assert!(!hashes.iter().any(|h| *h == "phantom_block_aaa"));
        assert!(!hashes.iter().any(|h| *h == "phantom_block_bbb"));
    }

    // -----------------------------------------------------------------------
    // Issue #1038: Tests exercising the ConsensusEngine trait interface
    // -----------------------------------------------------------------------

    /// Create a block via the `ConsensusEngine` trait, then validate it via the
    /// same trait interface. This is the basic round-trip that was previously
    /// untested.
    #[tokio::test]
    async fn dag_trait_create_then_validate_roundtrip() {
        let engine = DagConsensusEngine::new(10);
        let chain = Blockchain::default();

        // Create a block through the trait interface
        let block = engine
            .create_block(
                Vec::new(),
                &wrfcoin_shared_protocol::crypto::hash::Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();

        // The block must have a hash and the DAG metadata
        assert!(block.hash.is_some());
        assert_eq!(
            block.metadata.get("consensus_type").map(|s| s.as_str()),
            Some("DAG")
        );
        assert!(block.metadata.contains_key("dag_hash"));

        // Validate through the trait interface — the DAG engine should accept
        // its own block.
        let valid = engine.validate_block(&block, &chain).await.unwrap();
        assert!(valid, "DAG engine must validate its own block");
    }

    /// After creating a valid block, tamper with its hash. The DAG engine
    /// should reject the tampered block because the `dag_hash` metadata no
    /// longer matches an actual DAG block hash (the linear block hash is
    /// irrelevant to DAG validation — it's the `dag_hash` key that matters).
    #[tokio::test]
    async fn dag_trait_rejects_tampered_block() {
        let engine = DagConsensusEngine::new(10);
        let chain = Blockchain::default();

        let mut block = engine
            .create_block(
                Vec::new(),
                &wrfcoin_shared_protocol::crypto::hash::Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();

        // Tamper with the dag_hash metadata so the DAG lookup fails
        block
            .metadata
            .insert("dag_hash".into(), "tampered_hash_value".into());

        let valid = engine.validate_block(&block, &chain).await.unwrap();
        assert!(!valid, "DAG engine must reject a block with a tampered dag_hash");
    }

    /// A block with a non-DAG `consensus_type` must be rejected.
    #[tokio::test]
    async fn dag_trait_rejects_non_dag_consensus_type() {
        let engine = DagConsensusEngine::new(10);
        let chain = Blockchain::default();

        let mut block = engine
            .create_block(
                Vec::new(),
                &wrfcoin_shared_protocol::crypto::hash::Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();

        // Override consensus_type to something else
        block
            .metadata
            .insert("consensus_type".into(), "ProofOfWork".into());

        let valid = engine.validate_block(&block, &chain).await.unwrap();
        assert!(!valid, "DAG engine must reject non-DAG consensus_type blocks");
    }

    /// Use `ConsensusFactory::create(DAG, ...)` to instantiate the engine,
    /// then create and validate a block. Tests the factory → trait round-trip.
    #[tokio::test]
    async fn dag_trait_factory_create_and_validate() {
        use crate::factory::ConsensusFactory;
        use crate::types::ConsensusType;

        let config = ConsensusFactory::default_config(ConsensusType::DAG);
        let engine = ConsensusFactory::create(ConsensusType::DAG, &config).unwrap();

        assert_eq!(engine.algorithm_name(), "DAG");

        let chain = Blockchain::default();
        let block = engine
            .create_block(
                Vec::new(),
                &wrfcoin_shared_protocol::crypto::hash::Hash::default().to_string(),
                "factory-validator",
                &chain,
            )
            .await
            .unwrap();

        let valid = engine.validate_block(&block, &chain).await.unwrap();
        assert!(valid, "Factory-created DAG engine must validate its own block");
    }

    /// Create two blocks in sequence through the trait to verify that DAG
    /// state evolves correctly (tips update, heights increase).
    #[tokio::test]
    async fn dag_trait_sequential_blocks() {
        let engine = DagConsensusEngine::new(10);
        let mut chain = Blockchain::default();

        let block1 = engine
            .create_block(
                Vec::new(),
                &wrfcoin_shared_protocol::crypto::hash::Hash::default().to_string(),
                "validator-1",
                &chain,
            )
            .await
            .unwrap();

        // Add block1 to chain so next create_block sees updated height
        chain.add_block(block1.clone()).unwrap();

        let block2 = engine
            .create_block(
                Vec::new(),
                &block1.hash.unwrap().to_string(),
                "validator-2",
                &chain,
            )
            .await
            .unwrap();

        // Both blocks must be valid
        let valid1 = engine.validate_block(&block1, &chain).await.unwrap();
        let valid2 = engine.validate_block(&block2, &chain).await.unwrap();
        assert!(valid1, "First block must be valid");
        assert!(valid2, "Second block must be valid");

        // Heights must differ
        assert_eq!(block1.height, 1);
        assert_eq!(block2.height, 2);
    }
}
