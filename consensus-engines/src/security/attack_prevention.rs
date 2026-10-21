//! Attack Prevention and Safeguards for Consensus Mechanisms
//!
//! This module implements comprehensive protection mechanisms against various
//! attack vectors including Sybil attacks, double-spending, DoS attacks,
//! and other malicious behaviors targeting consensus algorithms.

use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use sha2::{Digest, Sha256};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;

use crate::types::{ConsensusError, ConsensusResult};

/// Configuration for attack prevention mechanisms
#[derive(Debug, Clone)]
pub struct AttackPreventionConfig {
    /// Enable Sybil attack protection
    pub enable_sybil_protection: bool,
    /// Enable double-spending protection
    pub enable_double_spend_protection: bool,
    /// Enable DoS protection
    pub enable_dos_protection: bool,
    /// Enable eclipse attack protection
    pub enable_eclipse_protection: bool,
    /// Enable long-range attack protection
    pub enable_long_range_protection: bool,

    // Sybil Protection Settings
    /// Minimum stake required to participate
    pub min_stake_amount: u128,
    /// Maximum stake concentration allowed (0.0 - 1.0)
    pub max_stake_concentration: f64,
    /// Require proof of personhood
    pub require_proof_of_personhood: bool,

    // DoS Protection Settings
    /// Maximum transactions per second per node
    pub max_tx_per_second: u32,
    /// Maximum block size in bytes
    pub max_block_size: usize,
    /// Penalty for exceeding limits
    pub dos_penalty_duration: Duration,

    // Double-Spending Protection Settings
    /// Minimum confirmations required
    pub min_confirmations: u32,
    /// Enable UTXO tracking
    pub enable_utxo_tracking: bool,
    /// Maximum reorg depth allowed
    pub max_reorg_depth: u32,

    // Eclipse Attack Protection
    /// Minimum diverse connections required
    pub min_diverse_connections: u32,
    /// Maximum connections from single IP subnet
    pub max_connections_per_subnet: u32,

    // General Security Settings
    /// Enable reputation system
    pub enable_reputation_system: bool,
    /// Enable byzantine fault tolerance
    pub enable_bft: bool,
    /// Maximum malicious node tolerance (0.0 - 1.0)
    pub max_malicious_threshold: f64,
}

impl Default for AttackPreventionConfig {
    fn default() -> Self {
        Self {
            enable_sybil_protection: true,
            enable_double_spend_protection: true,
            enable_dos_protection: true,
            enable_eclipse_protection: true,
            enable_long_range_protection: true,

            min_stake_amount: 1_000_000, // 1M tokens minimum
            max_stake_concentration: 0.10, // No single entity > 10% (PRD requirement)
            require_proof_of_personhood: false,

            max_tx_per_second: 100,
            max_block_size: 2 * 1024 * 1024,                 // 2MB
            dos_penalty_duration: Duration::from_secs(3600), // 1 hour

            min_confirmations: 6,
            enable_utxo_tracking: true,
            max_reorg_depth: 100,

            min_diverse_connections: 8,
            max_connections_per_subnet: 3,

            enable_reputation_system: true,
            enable_bft: true,
            max_malicious_threshold: 0.33, // 33% Byzantine tolerance
        }
    }
}

/// Node reputation score and tracking
#[derive(Debug, Clone)]
pub struct NodeReputation {
    /// Node identifier
    pub node_id: String,
    /// Current reputation score (0.0 - 1.0)
    pub score: f64,
    /// Total number of blocks validated
    pub blocks_validated: u64,
    /// Number of invalid blocks proposed
    pub invalid_blocks: u64,
    /// Last activity timestamp
    pub last_activity: u64,
    /// Stake amount
    pub stake: u128,
    /// Number of penalties received
    pub penalties: u32,
    /// Connection diversity score
    pub connection_diversity: f64,
}

impl Default for NodeReputation {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            score: 0.5, // Start with neutral reputation
            blocks_validated: 0,
            invalid_blocks: 0,
            last_activity: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            stake: 0,
            penalties: 0,
            connection_diversity: 0.0,
        }
    }
}

/// DoS protection state for a node
#[derive(Debug, Clone, Default)]
struct DosProtectionState {
    /// Transaction timestamps in current window
    tx_timestamps: VecDeque<u64>,
    /// Number of violations
    violations: u32,
    /// Penalty expiry timestamp
    penalty_until: Option<u64>,
    /// Last violation timestamp
    #[allow(dead_code)]
    last_violation: Option<u64>,
}

/// Double-spending protection using UTXO tracking
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct UtxoTracker {
    /// Unspent transaction outputs
    utxos: FxHashMap<String, UtxoEntry>,
    /// Spent outputs (for recent history)
    recent_spends: FxHashMap<String, u64>, // output_id -> block_height
    /// Pending transactions being validated
    pending_txs: FxHashSet<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct UtxoEntry {
    /// Transaction output amount
    amount: u64,
    /// Block height when created
    block_height: u64,
    /// Owner address
    owner: String,
    /// Timestamp when created
    timestamp: u64,
}

/// Main attack prevention system
#[derive(Debug)]
#[allow(dead_code)]
pub struct AttackPreventionSystem {
    /// Configuration
    config: AttackPreventionConfig,
    /// Node reputation tracking
    node_reputations: Arc<RwLock<FxHashMap<String, NodeReputation>>>,
    /// DoS protection state
    dos_protection: Arc<RwLock<FxHashMap<String, DosProtectionState>>>,
    /// UTXO tracker for double-spending prevention
    utxo_tracker: Arc<RwLock<UtxoTracker>>,
    /// Known malicious nodes
    malicious_nodes: Arc<RwLock<FxHashSet<String>>>,
    /// Network connection tracking
    connection_tracker: Arc<RwLock<FxHashMap<String, ConnectionInfo>>>,
    /// Stake concentration tracking
    stake_tracker: Arc<RwLock<FxHashMap<String, u128>>>,
    /// Pool membership tracking: pool_id -> set of node_ids
    pool_tracker: Arc<RwLock<FxHashMap<String, FxHashSet<String>>>>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ConnectionInfo {
    /// IP address
    ip_address: String,
    /// Connection timestamp
    connected_at: u64,
    /// Number of connections from this IP subnet
    subnet_connections: u32,
    /// Geographic location (if available)
    location: Option<String>,
}

impl AttackPreventionSystem {
    /// Create a new attack prevention system
    pub fn new(config: AttackPreventionConfig) -> Self {
        Self {
            config,
            node_reputations: Arc::new(RwLock::new(FxHashMap::default())),
            dos_protection: Arc::new(RwLock::new(FxHashMap::default())),
            utxo_tracker: Arc::new(RwLock::new(UtxoTracker {
                utxos: FxHashMap::default(),
                recent_spends: FxHashMap::default(),
                pending_txs: FxHashSet::default(),
            })),
            malicious_nodes: Arc::new(RwLock::new(FxHashSet::default())),
            connection_tracker: Arc::new(RwLock::new(FxHashMap::default())),
            stake_tracker: Arc::new(RwLock::new(FxHashMap::default())),
            pool_tracker: Arc::new(RwLock::new(FxHashMap::default())),
        }
    }

    /// Validate a transaction against all attack prevention mechanisms
    pub async fn validate_transaction(
        &self,
        transaction: &Transaction,
        validator_id: &str,
    ) -> ConsensusResult<bool> {
        // Check if validator is known to be malicious
        if self.is_malicious_node(validator_id).await {
            return Err(ConsensusError::MaliciousValidator(validator_id.to_string()));
        }

        // DoS protection check
        if self.config.enable_dos_protection && !self.check_dos_limits(validator_id).await? {
            return Ok(false);
        }

        // Double-spending protection check
        if self.config.enable_double_spend_protection
            && !self.check_double_spending(transaction).await?
        {
            return Ok(false);
        }

        // Reputation-based validation
        if self.config.enable_reputation_system {
            let reputation = self.get_node_reputation(validator_id).await;
            if reputation.score < 0.1 {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Validate a block against attack prevention mechanisms
    pub async fn validate_block(&self, block: &Block, proposer_id: &str) -> ConsensusResult<bool> {
        // Check if proposer is malicious
        if self.is_malicious_node(proposer_id).await {
            return Err(ConsensusError::MaliciousValidator(proposer_id.to_string()));
        }

        // Check block size limits (DoS protection)
        if self.config.enable_dos_protection {
            let block_size = self.estimate_block_size(block);
            if block_size > self.config.max_block_size {
                self.penalize_node(proposer_id, "Block size limit exceeded")
                    .await;
                return Ok(false);
            }
        }

        // Reputation check for block proposer
        if self.config.enable_reputation_system {
            let reputation = self.get_node_reputation(proposer_id).await;
            if reputation.score < 0.2 {
                return Ok(false);
            }
        }

        // Check stake concentration (Sybil protection)
        if self.config.enable_sybil_protection
            && !self.check_stake_concentration(proposer_id).await?
        {
            return Ok(false);
        }

        Ok(true)
    }

    /// Check DoS rate limits for a node
    async fn check_dos_limits(&self, node_id: &str) -> ConsensusResult<bool> {
        let mut dos_protection = self.dos_protection.write().await;
        let state = dos_protection.entry(node_id.to_string()).or_default();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Check if node is currently penalized
        if let Some(penalty_until) = state.penalty_until {
            if now < penalty_until {
                return Ok(false);
            } else {
                state.penalty_until = None;
            }
        }

        // Clean old timestamps (keep only last second)
        while let Some(&front_time) = state.tx_timestamps.front() {
            if now - front_time > 1 {
                state.tx_timestamps.pop_front();
            } else {
                break;
            }
        }

        // Check rate limit
        if state.tx_timestamps.len() >= self.config.max_tx_per_second as usize {
            state.violations += 1;
            state.last_violation = Some(now);

            // Apply penalty
            let penalty_duration = self.config.dos_penalty_duration.as_secs();
            state.penalty_until = Some(now + penalty_duration);

            // Mark as malicious if too many violations
            if state.violations >= 5 {
                self.mark_malicious_node(node_id).await;
            }

            return Ok(false);
        }

        // Record this transaction
        state.tx_timestamps.push_back(now);
        Ok(true)
    }

    /// Check for double-spending attempts
    async fn check_double_spending(&self, transaction: &Transaction) -> ConsensusResult<bool> {
        if !self.config.enable_utxo_tracking {
            return Ok(true);
        }

        let mut utxo_tracker = self.utxo_tracker.write().await;
        let tx_hash = self.calculate_transaction_hash(transaction);

        // Check if this transaction is already pending
        if utxo_tracker.pending_txs.contains(&tx_hash) {
            return Ok(false);
        }

        // For each input, check if UTXO exists and hasn't been spent
        for input in &transaction.inputs {
            let input_key = format!("{}:{}", hex::encode(input.txid), input.vout);

            // Check if UTXO exists
            if !utxo_tracker.utxos.contains_key(&input_key) {
                return Ok(false);
            }

            // Check if already spent recently
            if utxo_tracker.recent_spends.contains_key(&input_key) {
                return Ok(false);
            }
        }

        // Mark transaction as pending
        utxo_tracker.pending_txs.insert(tx_hash);
        Ok(true)
    }

    /// Check stake concentration to prevent Sybil attacks (individual + pool-level)
    async fn check_stake_concentration(&self, node_id: &str) -> ConsensusResult<bool> {
        let stake_tracker = self.stake_tracker.read().await;

        // Calculate total stake
        let total_stake: u128 = stake_tracker.values().sum();

        if total_stake == 0 {
            return Ok(true);
        }

        // Check node's stake
        let node_stake = stake_tracker.get(node_id).copied().unwrap_or(0);

        // Check minimum stake requirement
        if node_stake < self.config.min_stake_amount {
            return Ok(false);
        }

        // Check individual stake concentration
        let stake_percentage = node_stake as f64 / total_stake as f64;
        if stake_percentage > self.config.max_stake_concentration {
            return Ok(false);
        }

        // Check pool-level stake concentration
        let pool_tracker = self.pool_tracker.read().await;
        for (_pool_id, members) in pool_tracker.iter() {
            if members.contains(node_id) {
                let pool_stake: u128 = members
                    .iter()
                    .filter_map(|id| stake_tracker.get(id))
                    .sum();
                let pool_percentage = pool_stake as f64 / total_stake as f64;
                if pool_percentage > self.config.max_stake_concentration {
                    return Ok(false);
                }
                break;
            }
        }

        Ok(true)
    }

    /// Update pool membership for a node.
    /// Pass `None` to remove the node from any pool.
    pub async fn update_node_pool(&self, node_id: &str, pool_id: Option<&str>) {
        let mut pool_tracker = self.pool_tracker.write().await;

        // Remove node from any existing pool
        for members in pool_tracker.values_mut() {
            members.remove(node_id);
        }
        // Clean up empty pools
        pool_tracker.retain(|_, members| !members.is_empty());

        // Add to new pool if specified
        if let Some(pid) = pool_id {
            pool_tracker
                .entry(pid.to_string())
                .or_default()
                .insert(node_id.to_string());
        }
    }

    /// Update node reputation based on behavior
    pub async fn update_node_reputation(&self, node_id: &str, behavior: NodeBehavior) {
        let mut reputations = self.node_reputations.write().await;
        let reputation = reputations.entry(node_id.to_string()).or_default();
        reputation.node_id = node_id.to_string();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        reputation.last_activity = now;

        match behavior {
            NodeBehavior::ValidBlock => {
                reputation.blocks_validated += 1;
                reputation.score = (reputation.score * 0.95 + 0.05).min(1.0);
            },
            NodeBehavior::InvalidBlock => {
                reputation.invalid_blocks += 1;
                reputation.score = (reputation.score * 0.9 - 0.1).max(0.0);
                reputation.penalties += 1;
            },
            NodeBehavior::MissedValidation => {
                reputation.score = (reputation.score * 0.98 - 0.02).max(0.0);
            },
            NodeBehavior::DoSAttempt => {
                reputation.score = (reputation.score * 0.8 - 0.2).max(0.0);
                reputation.penalties += 2;
            },
            NodeBehavior::MaliciousActivity => {
                reputation.score = 0.0;
                reputation.penalties += 10;
            },
        }

        // Auto-ban nodes with very low reputation
        if reputation.score < 0.05 || reputation.penalties > 20 {
            drop(reputations);
            self.mark_malicious_node(node_id).await;
        }
    }

    /// Get node reputation
    pub async fn get_node_reputation(&self, node_id: &str) -> NodeReputation {
        let reputations = self.node_reputations.read().await;
        reputations.get(node_id).cloned().unwrap_or_else(|| {
            NodeReputation { node_id: node_id.to_string(), ..Default::default() }
        })
    }

    /// Mark a node as malicious
    pub async fn mark_malicious_node(&self, node_id: &str) {
        let mut malicious_nodes = self.malicious_nodes.write().await;
        malicious_nodes.insert(node_id.to_string());

        // Also update reputation to zero (create entry if missing)
        let mut reputations = self.node_reputations.write().await;
        let reputation = reputations.entry(node_id.to_string()).or_insert_with(|| {
            NodeReputation { node_id: node_id.to_string(), ..Default::default() }
        });
        reputation.score = 0.0;
    }

    /// Check if a node is known to be malicious
    pub async fn is_malicious_node(&self, node_id: &str) -> bool {
        let malicious_nodes = self.malicious_nodes.read().await;
        malicious_nodes.contains(node_id)
    }

    /// Apply penalty to a node
    async fn penalize_node(&self, node_id: &str, _reason: &str) {
        self.update_node_reputation(node_id, NodeBehavior::DoSAttempt)
            .await;

        let mut dos_protection = self.dos_protection.write().await;
        let state = dos_protection.entry(node_id.to_string()).or_default();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        state.violations += 1;
        state.penalty_until = Some(now + self.config.dos_penalty_duration.as_secs());
    }

    /// Process a confirmed transaction (update UTXO set)
    pub async fn process_confirmed_transaction(
        &self,
        transaction: &Transaction,
        block_height: u64,
    ) {
        if !self.config.enable_utxo_tracking {
            return;
        }

        let mut utxo_tracker = self.utxo_tracker.write().await;
        let tx_hash = self.calculate_transaction_hash(transaction);

        // Remove from pending
        utxo_tracker.pending_txs.remove(&tx_hash);

        // Spend inputs
        for input in &transaction.inputs {
            let input_key = format!("{}:{}", hex::encode(input.txid), input.vout);
            if utxo_tracker.utxos.remove(&input_key).is_some() {
                utxo_tracker.recent_spends.insert(input_key, block_height);
            }
        }

        // Create new UTXOs
        for (index, output) in transaction.outputs.iter().enumerate() {
            let output_key = format!("{tx_hash}:{index}");
            let utxo_entry = UtxoEntry {
                amount: output.amount,
                block_height,
                owner: output.address.clone(),
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            };
            utxo_tracker.utxos.insert(output_key, utxo_entry);
        }

        // Clean old recent spends (keep only last 1000 blocks)
        utxo_tracker
            .recent_spends
            .retain(|_, &mut spent_height| block_height - spent_height < 1000);
    }

    /// Update node stake (for stake-based Sybil protection)
    pub async fn update_node_stake(&self, node_id: &str, stake: u128) {
        let mut stake_tracker = self.stake_tracker.write().await;
        stake_tracker.insert(node_id.to_string(), stake);
    }

    /// Get network health metrics
    pub async fn get_network_health(&self) -> NetworkHealthMetrics {
        let reputations = self.node_reputations.read().await;
        let malicious_nodes = self.malicious_nodes.read().await;
        let stake_tracker = self.stake_tracker.read().await;

        let total_nodes = reputations.len();
        let malicious_count = malicious_nodes.len();
        let avg_reputation = if total_nodes > 0 {
            reputations.values().map(|r| r.score).sum::<f64>() / total_nodes as f64
        } else {
            0.0
        };

        let total_stake: u128 = stake_tracker.values().sum();
        let max_stake = stake_tracker.values().max().copied().unwrap_or(0);
        let stake_concentration = if total_stake > 0 {
            max_stake as f64 / total_stake as f64
        } else {
            0.0
        };

        NetworkHealthMetrics {
            total_nodes,
            malicious_nodes: malicious_count,
            avg_reputation,
            stake_concentration,
            byzantine_tolerance: (total_nodes.saturating_sub(malicious_count * 3)) > 0,
        }
    }

    /// Helper methods
    fn estimate_block_size(&self, block: &Block) -> usize {
        // Simplified block size estimation
        let base_size = 80; // Block header size
        let tx_size: usize = block
            .transactions
            .iter()
            .map(|tx| tx.inputs.len() * 40 + tx.outputs.len() * 34 + 10)
            .sum();
        base_size + tx_size
    }

    fn calculate_transaction_hash(&self, transaction: &Transaction) -> String {
        // Simplified transaction hash calculation using SHA256
        let mut hasher = Sha256::new();
        hasher.update(format!("{transaction:?}").as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

/// Node behavior types for reputation scoring
#[derive(Debug, Clone, Copy)]
pub enum NodeBehavior {
    /// Node validated a valid block
    ValidBlock,
    /// Node proposed/validated an invalid block
    InvalidBlock,
    /// Node missed a validation opportunity
    MissedValidation,
    /// Node attempted DoS attack
    DoSAttempt,
    /// Node engaged in malicious activity
    MaliciousActivity,
}

/// Network health metrics
#[derive(Debug, Clone)]
pub struct NetworkHealthMetrics {
    /// Total number of nodes
    pub total_nodes: usize,
    /// Number of malicious nodes detected
    pub malicious_nodes: usize,
    /// Average reputation score
    pub avg_reputation: f64,
    /// Stake concentration (highest stake percentage)
    pub stake_concentration: f64,
    /// Whether network maintains Byzantine fault tolerance
    pub byzantine_tolerance: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dos_protection() {
        let config = AttackPreventionConfig {
            max_tx_per_second: 2,
            ..Default::default()
        };
        let system = AttackPreventionSystem::new(config);

        let node_id = "test_node";

        // First two transactions should pass
        assert!(system.check_dos_limits(node_id).await.unwrap());
        assert!(system.check_dos_limits(node_id).await.unwrap());

        // Third should fail (exceeds limit)
        assert!(!system.check_dos_limits(node_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_reputation_system() {
        let system = AttackPreventionSystem::new(AttackPreventionConfig::default());
        let node_id = "test_node";

        // Initial reputation should be neutral
        let initial_rep = system.get_node_reputation(node_id).await;
        assert_eq!(initial_rep.score, 0.5);

        // Valid block should improve reputation
        system
            .update_node_reputation(node_id, NodeBehavior::ValidBlock)
            .await;
        let improved_rep = system.get_node_reputation(node_id).await;
        assert!(improved_rep.score > initial_rep.score);

        // Invalid block should decrease reputation
        system
            .update_node_reputation(node_id, NodeBehavior::InvalidBlock)
            .await;
        let decreased_rep = system.get_node_reputation(node_id).await;
        assert!(decreased_rep.score < improved_rep.score);
    }

    #[tokio::test]
    async fn test_malicious_node_marking() {
        let system = AttackPreventionSystem::new(AttackPreventionConfig::default());
        let node_id = "malicious_node";

        assert!(!system.is_malicious_node(node_id).await);

        system.mark_malicious_node(node_id).await;

        assert!(system.is_malicious_node(node_id).await);

        let reputation = system.get_node_reputation(node_id).await;
        assert_eq!(reputation.score, 0.0);
    }

    #[tokio::test]
    async fn test_stake_concentration() {
        // Default max_stake_concentration = 0.10 (10%), min_stake_amount = 1_000_000
        let system = AttackPreventionSystem::new(AttackPreventionConfig::default());

        // Setup 12 nodes so each stays below 10%: total = 12M, each ~8.3%
        for i in 1..=12 {
            system
                .update_node_stake(&format!("node{i}"), 1_000_000)
                .await;
        }

        // All nodes should pass (each ~8.3% < 10%)
        assert!(system.check_stake_concentration("node1").await.unwrap());
        assert!(system.check_stake_concentration("node6").await.unwrap());
        assert!(system.check_stake_concentration("node12").await.unwrap());

        // Add a whale with overwhelming stake to exceed 10%
        system.update_node_stake("whale", 50_000_000).await;
        assert!(!system.check_stake_concentration("whale").await.unwrap());
    }

    #[tokio::test]
    async fn test_pool_stake_concentration() {
        let system = AttackPreventionSystem::new(AttackPreventionConfig::default());

        // 10 nodes at 1M each = 10M total, each at 10%
        for i in 1..=10 {
            system
                .update_node_stake(&format!("node{i}"), 1_000_000)
                .await;
        }

        // Individual nodes at exactly 10% pass (uses > not >=)
        assert!(system.check_stake_concentration("node1").await.unwrap());

        // Group 3 nodes into pool-A: aggregate = 3M/10M = 30% > 10%
        system.update_node_pool("node1", Some("pool-A")).await;
        system.update_node_pool("node2", Some("pool-A")).await;
        system.update_node_pool("node3", Some("pool-A")).await;

        // Pool members should now fail concentration check
        assert!(!system.check_stake_concentration("node1").await.unwrap());
        assert!(!system.check_stake_concentration("node2").await.unwrap());

        // Node not in any pool should still pass individually
        assert!(system.check_stake_concentration("node5").await.unwrap());

        // Remove node1 from pool, it should pass again
        system.update_node_pool("node1", None).await;
        assert!(system.check_stake_concentration("node1").await.unwrap());

        // Pool-A now has 2 nodes (2M/10M = 20% > 10%) — still fails
        assert!(!system.check_stake_concentration("node2").await.unwrap());
    }
}
