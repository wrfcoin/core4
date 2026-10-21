//! Consensus-Level Sharding
//!
//! Implements a sharded consensus architecture where the validator set is
//! partitioned into shards, each responsible for processing a subset of
//! transactions. A beacon chain coordinates all shards, aggregates state roots,
//! and routes cross-shard messages with receipt proofs.
//!
//! Shard assignment uses VRF-based randomness to prevent adversarial
//! concentration, and validators are rotated across shards each epoch.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;

/// Default number of shards in the network.
pub const DEFAULT_NUM_SHARDS: u32 = 16;

/// Default number of validators per shard.
pub const DEFAULT_VALIDATORS_PER_SHARD: usize = 32;

/// Default epoch length in blocks.
pub const DEFAULT_EPOCH_LENGTH: u64 = 256;

/// Default timeout (in blocks) for cross-shard messages.
pub const DEFAULT_CROSS_SHARD_TIMEOUT: u64 = 64;

/// Errors that can occur in the sharding subsystem.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ShardingError {
    #[error("Shard not found: {0}")]
    ShardNotFound(u32),

    #[error("Validator not found: {0}")]
    ValidatorNotFound(String),

    #[error("Validator already assigned: {0}")]
    ValidatorAlreadyAssigned(String),

    #[error("Shard capacity exceeded: shard {shard_id} has {current}/{max} validators")]
    ShardCapacityExceeded {
        shard_id: u32,
        current: usize,
        max: usize,
    },

    #[error(
        "Cross-shard message timeout: message {message_id} from shard {source_shard} to shard {dest_shard}"
    )]
    CrossShardTimeout {
        message_id: String,
        source_shard: u32,
        dest_shard: u32,
    },

    #[error("Invalid receipt proof for message {0}")]
    InvalidReceiptProof(String),

    #[error("Epoch mismatch: expected {expected}, got {actual}")]
    EpochMismatch { expected: u64, actual: u64 },

    #[error("Insufficient validators: need {need}, have {have}")]
    InsufficientValidators { need: usize, have: usize },

    #[error("Invalid shard configuration: {0}")]
    InvalidConfig(String),
}

/// Configuration for the sharding subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardConfig {
    /// Number of shards in the network.
    pub num_shards: u32,
    /// Target number of validators per shard.
    pub validators_per_shard: usize,
    /// Number of blocks per epoch (shard rotation happens at epoch boundaries).
    pub epoch_length: u64,
    /// Timeout in blocks for cross-shard message delivery.
    pub cross_shard_timeout: u64,
    /// Minimum number of validators required per shard for safety.
    pub min_validators_per_shard: usize,
}

impl Default for ShardConfig {
    fn default() -> Self {
        Self {
            num_shards: DEFAULT_NUM_SHARDS,
            validators_per_shard: DEFAULT_VALIDATORS_PER_SHARD,
            epoch_length: DEFAULT_EPOCH_LENGTH,
            cross_shard_timeout: DEFAULT_CROSS_SHARD_TIMEOUT,
            min_validators_per_shard: 4,
        }
    }
}

/// A validator assigned to a specific shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardValidator {
    /// Validator address/identifier.
    pub address: String,
    /// Validator's staked amount.
    pub stake: u64,
    /// Shard ID this validator is currently assigned to.
    pub assigned_shard: Option<u32>,
    /// Performance score (0-100) based on uptime and block production.
    pub performance_score: u8,
    /// Number of blocks produced by this validator.
    pub blocks_produced: u64,
    /// Number of missed slots.
    pub missed_slots: u64,
    /// Epoch when the current shard assignment was made.
    pub assignment_epoch: u64,
}

impl ShardValidator {
    /// Create a new shard validator.
    pub fn new(address: String, stake: u64) -> Self {
        Self {
            address,
            stake,
            assigned_shard: None,
            performance_score: 100,
            blocks_produced: 0,
            missed_slots: 0,
            assignment_epoch: 0,
        }
    }
}

/// The state of a single shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shard {
    /// Unique shard identifier.
    pub shard_id: u32,
    /// Addresses of validators assigned to this shard.
    pub validators: Vec<String>,
    /// Merkle root of the shard's state.
    pub state_root: String,
    /// Current block height within this shard.
    pub block_height: u64,
    /// Pending cross-shard messages awaiting delivery.
    pub pending_cross_shard_msgs: VecDeque<CrossShardMessage>,
    /// Number of transactions processed by this shard.
    pub transactions_processed: u64,
}

impl Shard {
    /// Create a new empty shard.
    pub fn new(shard_id: u32) -> Self {
        Self {
            shard_id,
            validators: Vec::new(),
            state_root: String::new(),
            block_height: 0,
            pending_cross_shard_msgs: VecDeque::new(),
            transactions_processed: 0,
        }
    }

    /// Add a validator to this shard.
    pub fn add_validator(
        &mut self,
        address: String,
        max_validators: usize,
    ) -> Result<(), ShardingError> {
        if self.validators.len() >= max_validators {
            return Err(ShardingError::ShardCapacityExceeded {
                shard_id: self.shard_id,
                current: self.validators.len(),
                max: max_validators,
            });
        }

        if self.validators.contains(&address) {
            return Err(ShardingError::ValidatorAlreadyAssigned(address));
        }

        self.validators.push(address);
        Ok(())
    }

    /// Remove a validator from this shard.
    pub fn remove_validator(&mut self, address: &str) -> bool {
        if let Some(pos) = self.validators.iter().position(|a| a == address) {
            self.validators.remove(pos);
            true
        } else {
            false
        }
    }

    /// Update the state root after processing a block.
    pub fn update_state_root(&mut self, new_root: String) {
        self.state_root = new_root;
        self.block_height += 1;
    }
}

/// A cross-shard message with a receipt proof for verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossShardMessage {
    /// Unique message identifier.
    pub message_id: String,
    /// Source shard ID.
    pub source_shard: u32,
    /// Destination shard ID.
    pub dest_shard: u32,
    /// Message payload (serialized transaction or state update).
    pub payload: Vec<u8>,
    /// Merkle proof that the message was included in the source shard's state.
    pub receipt_proof: ReceiptProof,
    /// Block height at which the message was created.
    pub created_at_height: u64,
    /// Timestamp of message creation.
    pub timestamp: u64,
    /// Whether the message has been delivered.
    pub delivered: bool,
}

/// Proof that a cross-shard message was committed in the source shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptProof {
    /// State root of the source shard at the time the message was created.
    pub state_root: String,
    /// Merkle proof path (list of sibling hashes).
    pub proof_path: Vec<String>,
    /// Index of the message in the source shard's outbox.
    pub message_index: u64,
}

/// Routes messages between shards and tracks delivery status.
#[derive(Debug, Clone)]
pub struct CrossShardRouter {
    /// Pending messages indexed by destination shard.
    pending_by_dest: FxHashMap<u32, Vec<CrossShardMessage>>,
    /// All messages indexed by message ID.
    messages: FxHashMap<String, CrossShardMessage>,
    /// Messages that have timed out.
    timed_out: Vec<String>,
    /// Cross-shard timeout in blocks.
    timeout_blocks: u64,
    /// Counter for generating message IDs.
    next_message_id: u64,
}

impl CrossShardRouter {
    /// Create a new cross-shard router.
    pub fn new(timeout_blocks: u64) -> Self {
        Self {
            pending_by_dest: FxHashMap::default(),
            messages: FxHashMap::default(),
            timed_out: Vec::new(),
            timeout_blocks,
            next_message_id: 1,
        }
    }

    /// Create and enqueue a new cross-shard message.
    pub fn send_message(
        &mut self,
        source_shard: u32,
        dest_shard: u32,
        payload: Vec<u8>,
        source_state_root: String,
        current_height: u64,
        timestamp: u64,
    ) -> String {
        let message_id = format!(
            "xshard-{}-{}-{}",
            source_shard, dest_shard, self.next_message_id
        );
        self.next_message_id += 1;

        let proof = ReceiptProof {
            state_root: source_state_root,
            proof_path: vec![], // Simplified; real implementation would compute Merkle proof
            message_index: self.next_message_id - 1,
        };

        let msg = CrossShardMessage {
            message_id: message_id.clone(),
            source_shard,
            dest_shard,
            payload,
            receipt_proof: proof,
            created_at_height: current_height,
            timestamp,
            delivered: false,
        };

        self.pending_by_dest
            .entry(dest_shard)
            .or_default()
            .push(msg.clone());
        self.messages.insert(message_id.clone(), msg);

        message_id
    }

    /// Deliver a pending message to its destination shard.
    pub fn deliver_message(
        &mut self,
        message_id: &str,
    ) -> Result<CrossShardMessage, ShardingError> {
        let msg = self
            .messages
            .get_mut(message_id)
            .ok_or_else(|| ShardingError::InvalidReceiptProof(message_id.to_string()))?;

        msg.delivered = true;
        let delivered_msg = msg.clone();

        // Remove from pending queue
        if let Some(pending) = self.pending_by_dest.get_mut(&delivered_msg.dest_shard) {
            pending.retain(|m| m.message_id != message_id);
        }

        Ok(delivered_msg)
    }

    /// Get pending messages for a specific destination shard.
    pub fn pending_for_shard(&self, shard_id: u32) -> Vec<&CrossShardMessage> {
        self.pending_by_dest
            .get(&shard_id)
            .map(|msgs| msgs.iter().collect())
            .unwrap_or_default()
    }

    /// Check for timed-out messages and move them to the timeout list.
    pub fn check_timeouts(&mut self, current_height: u64) -> Vec<String> {
        let mut newly_timed_out = Vec::new();

        for msg in self.messages.values() {
            if !msg.delivered
                && current_height > msg.created_at_height + self.timeout_blocks
                && !self.timed_out.contains(&msg.message_id)
            {
                newly_timed_out.push(msg.message_id.clone());
            }
        }

        self.timed_out.extend(newly_timed_out.clone());
        newly_timed_out
    }

    /// Get all timed-out message IDs.
    pub fn timed_out_messages(&self) -> &[String] {
        &self.timed_out
    }

    /// Get the total number of messages routed.
    pub fn total_messages(&self) -> usize {
        self.messages.len()
    }
}

/// Shard assignment result from VRF-based selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardAssignment {
    /// Epoch for which this assignment is valid.
    pub epoch: u64,
    /// Mapping of validator address to assigned shard ID.
    pub assignments: FxHashMap<String, u32>,
    /// VRF seed used for this assignment.
    pub vrf_seed: String,
}

/// A shard header included in the beacon chain for cross-linking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardHeader {
    /// Shard ID.
    pub shard_id: u32,
    /// State root of the shard.
    pub state_root: String,
    /// Block height of the shard.
    pub block_height: u64,
    /// Number of transactions in this shard block.
    pub transaction_count: u64,
    /// Hash of the shard header.
    pub header_hash: String,
}

impl ShardHeader {
    /// Compute the header hash.
    fn compute_hash(shard_id: u32, state_root: &str, block_height: u64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"wrfcoin-shard-header-v1|");
        hasher.update(shard_id.to_be_bytes());
        hasher.update(state_root.as_bytes());
        hasher.update(block_height.to_be_bytes());
        hex::encode(hasher.finalize())
    }
}

/// Beacon chain that coordinates all shards.
///
/// Aggregates shard state roots into a global state root, stores cross-links
/// to shard headers, and manages the global view of the sharded network.
#[derive(Debug, Clone)]
pub struct BeaconChain {
    /// Global state root aggregating all shard state roots.
    pub global_state_root: String,
    /// Current beacon chain block height.
    pub block_height: u64,
    /// Current epoch.
    pub epoch: u64,
    /// Shard headers for the current slot (cross-links).
    pub shard_headers: FxHashMap<u32, ShardHeader>,
    /// History of global state roots by height.
    state_root_history: Vec<String>,
}

impl BeaconChain {
    /// Create a new beacon chain.
    pub fn new() -> Self {
        Self {
            global_state_root: String::new(),
            block_height: 0,
            epoch: 0,
            shard_headers: FxHashMap::default(),
            state_root_history: Vec::new(),
        }
    }

    /// Submit a shard header (cross-link) to the beacon chain.
    pub fn submit_shard_header(
        &mut self,
        shard_id: u32,
        state_root: String,
        block_height: u64,
        transaction_count: u64,
    ) -> ShardHeader {
        let header_hash = ShardHeader::compute_hash(shard_id, &state_root, block_height);

        let header = ShardHeader {
            shard_id,
            state_root,
            block_height,
            transaction_count,
            header_hash,
        };

        self.shard_headers.insert(shard_id, header.clone());
        header
    }

    /// Aggregate all shard state roots into a global state root.
    ///
    /// The global state root is a hash of all shard state roots sorted by
    /// shard ID, ensuring deterministic computation.
    pub fn aggregate_state_roots(&mut self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"wrfcoin-global-state-v1|");
        hasher.update(self.block_height.to_be_bytes());

        // Sort by shard ID for deterministic ordering
        let mut shard_ids: Vec<u32> = self.shard_headers.keys().cloned().collect();
        shard_ids.sort();

        for shard_id in &shard_ids {
            if let Some(header) = self.shard_headers.get(shard_id) {
                hasher.update(shard_id.to_be_bytes());
                hasher.update(header.state_root.as_bytes());
            }
        }

        let root = hex::encode(hasher.finalize());
        self.global_state_root = root.clone();
        self.state_root_history.push(root.clone());
        self.block_height += 1;

        root
    }

    /// Advance to the next epoch.
    pub fn advance_epoch(&mut self) {
        self.epoch += 1;
    }

    /// Get the global state root history.
    pub fn state_root_history(&self) -> &[String] {
        &self.state_root_history
    }

    /// Get the shard header for a specific shard.
    pub fn get_shard_header(&self, shard_id: u32) -> Option<&ShardHeader> {
        self.shard_headers.get(&shard_id)
    }
}

impl Default for BeaconChain {
    fn default() -> Self {
        Self::new()
    }
}

/// Top-level shard manager responsible for shard lifecycle, validator
/// assignment, rotation, and coordination.
#[derive(Debug, Clone)]
pub struct ShardManager {
    /// Sharding configuration.
    config: ShardConfig,
    /// All shards indexed by shard ID.
    shards: FxHashMap<u32, Shard>,
    /// All registered validators.
    validators: FxHashMap<String, ShardValidator>,
    /// Beacon chain for shard coordination.
    beacon_chain: BeaconChain,
    /// Cross-shard message router.
    router: CrossShardRouter,
    /// Current epoch.
    current_epoch: u64,
    /// History of shard assignments by epoch.
    assignment_history: Vec<ShardAssignment>,
}

impl ShardManager {
    /// Create a new shard manager and initialize all shards.
    pub fn new(config: ShardConfig) -> Self {
        let mut shards = FxHashMap::default();
        for i in 0..config.num_shards {
            shards.insert(i, Shard::new(i));
        }

        let router = CrossShardRouter::new(config.cross_shard_timeout);

        Self {
            config,
            shards,
            validators: FxHashMap::default(),
            beacon_chain: BeaconChain::new(),
            router,
            current_epoch: 0,
            assignment_history: Vec::new(),
        }
    }

    /// Register a validator in the shard manager.
    pub fn register_validator(&mut self, address: String, stake: u64) -> Result<(), ShardingError> {
        if self.validators.contains_key(&address) {
            return Err(ShardingError::ValidatorAlreadyAssigned(address));
        }
        self.validators
            .insert(address.clone(), ShardValidator::new(address, stake));
        Ok(())
    }

    /// Assign validators to shards using VRF-based random assignment.
    ///
    /// The VRF seed (typically derived from the beacon chain randomness)
    /// determines the assignment. Validators are sorted by a hash of their
    /// address and the VRF seed, then distributed round-robin across shards.
    #[allow(clippy::excessive_nesting)]
    pub fn assign_validators(&mut self, vrf_seed: &str) -> Result<ShardAssignment, ShardingError> {
        let total_needed = self.config.min_validators_per_shard * self.config.num_shards as usize;
        if self.validators.len() < total_needed {
            return Err(ShardingError::InsufficientValidators {
                need: total_needed,
                have: self.validators.len(),
            });
        }

        // Clear existing assignments
        for shard in self.shards.values_mut() {
            shard.validators.clear();
        }
        for validator in self.validators.values_mut() {
            validator.assigned_shard = None;
        }

        // Sort validators by VRF-seeded hash for pseudo-random assignment
        let mut validator_order: Vec<(String, String)> = self
            .validators
            .keys()
            .map(|addr| {
                let mut hasher = Sha256::new();
                hasher.update(b"wrfcoin-shard-vrf-v1|");
                hasher.update(vrf_seed.as_bytes());
                hasher.update(addr.as_bytes());
                hasher.update(self.current_epoch.to_be_bytes());
                let sort_key = hex::encode(hasher.finalize());
                (addr.clone(), sort_key)
            })
            .collect();

        validator_order.sort_by(|a, b| a.1.cmp(&b.1));

        // Assign validators round-robin to shards
        let mut assignments = FxHashMap::default();
        for (i, (addr, _)) in validator_order.iter().enumerate() {
            let shard_id = (i as u32) % self.config.num_shards;

            if let Some(shard) = self.shards.get_mut(&shard_id) {
                // Only assign up to max validators per shard
                if shard.validators.len() < self.config.validators_per_shard {
                    shard.validators.push(addr.clone());
                    if let Some(validator) = self.validators.get_mut(addr) {
                        validator.assigned_shard = Some(shard_id);
                        validator.assignment_epoch = self.current_epoch;
                    }
                    assignments.insert(addr.clone(), shard_id);
                }
            }
        }

        let assignment = ShardAssignment {
            epoch: self.current_epoch,
            assignments,
            vrf_seed: vrf_seed.to_string(),
        };

        self.assignment_history.push(assignment.clone());
        Ok(assignment)
    }

    /// Rotate validators to new shards for the next epoch.
    ///
    /// Advances the epoch and re-assigns all validators using a new VRF seed.
    pub fn rotate_shards(&mut self, new_vrf_seed: &str) -> Result<ShardAssignment, ShardingError> {
        self.current_epoch += 1;
        self.beacon_chain.advance_epoch();
        self.assign_validators(new_vrf_seed)
    }

    /// Send a cross-shard message.
    pub fn send_cross_shard_message(
        &mut self,
        source_shard: u32,
        dest_shard: u32,
        payload: Vec<u8>,
        current_height: u64,
        timestamp: u64,
    ) -> Result<String, ShardingError> {
        let source = self
            .shards
            .get(&source_shard)
            .ok_or(ShardingError::ShardNotFound(source_shard))?;
        let state_root = source.state_root.clone();

        if !self.shards.contains_key(&dest_shard) {
            return Err(ShardingError::ShardNotFound(dest_shard));
        }

        let msg_id = self.router.send_message(
            source_shard,
            dest_shard,
            payload,
            state_root,
            current_height,
            timestamp,
        );

        Ok(msg_id)
    }

    /// Deliver a cross-shard message to its destination.
    pub fn deliver_cross_shard_message(
        &mut self,
        message_id: &str,
    ) -> Result<CrossShardMessage, ShardingError> {
        let msg = self.router.deliver_message(message_id)?;

        if let Some(shard) = self.shards.get_mut(&msg.dest_shard) {
            shard.pending_cross_shard_msgs.push_back(msg.clone());
        }

        Ok(msg)
    }

    /// Submit a shard block header to the beacon chain.
    pub fn submit_shard_block(
        &mut self,
        shard_id: u32,
        state_root: String,
        transaction_count: u64,
    ) -> Result<ShardHeader, ShardingError> {
        let shard = self
            .shards
            .get_mut(&shard_id)
            .ok_or(ShardingError::ShardNotFound(shard_id))?;

        shard.update_state_root(state_root.clone());
        shard.transactions_processed += transaction_count;

        let header = self.beacon_chain.submit_shard_header(
            shard_id,
            state_root,
            shard.block_height,
            transaction_count,
        );

        Ok(header)
    }

    /// Aggregate all shard state roots into a global state root on the beacon chain.
    pub fn aggregate_state(&mut self) -> String {
        self.beacon_chain.aggregate_state_roots()
    }

    /// Get a reference to a shard by ID.
    pub fn get_shard(&self, shard_id: u32) -> Option<&Shard> {
        self.shards.get(&shard_id)
    }

    /// Get a reference to the beacon chain.
    pub fn beacon_chain(&self) -> &BeaconChain {
        &self.beacon_chain
    }

    /// Get a reference to the cross-shard router.
    pub fn router(&self) -> &CrossShardRouter {
        &self.router
    }

    /// Get the current epoch.
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Get the shard assignment history.
    pub fn assignment_history(&self) -> &[ShardAssignment] {
        &self.assignment_history
    }

    /// Get the number of shards.
    pub fn num_shards(&self) -> u32 {
        self.config.num_shards
    }

    /// Get a reference to a validator by address.
    pub fn get_validator(&self, address: &str) -> Option<&ShardValidator> {
        self.validators.get(address)
    }

    /// Get all registered validators.
    pub fn all_validators(&self) -> &FxHashMap<String, ShardValidator> {
        &self.validators
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shard_manager(num_shards: u32, validators_per_shard: usize) -> ShardManager {
        let config = ShardConfig {
            num_shards,
            validators_per_shard,
            epoch_length: 256,
            cross_shard_timeout: 64,
            min_validators_per_shard: 2,
        };
        ShardManager::new(config)
    }

    fn register_validators(mgr: &mut ShardManager, count: usize, base_stake: u64) {
        for i in 0..count {
            mgr.register_validator(format!("validator-{}", i), base_stake + i as u64 * 100)
                .unwrap();
        }
    }

    #[test]
    fn test_shard_initialization() {
        let mgr = make_shard_manager(4, 8);

        assert_eq!(mgr.num_shards(), 4);
        for i in 0..4 {
            let shard = mgr.get_shard(i).unwrap();
            assert_eq!(shard.shard_id, i);
            assert!(shard.validators.is_empty());
            assert_eq!(shard.block_height, 0);
        }
    }

    #[test]
    fn test_validator_registration() {
        let mut mgr = make_shard_manager(4, 8);
        register_validators(&mut mgr, 16, 1000);

        assert_eq!(mgr.all_validators().len(), 16);

        // Duplicate registration should fail
        let result = mgr.register_validator("validator-0".to_string(), 1000);
        assert!(matches!(
            result,
            Err(ShardingError::ValidatorAlreadyAssigned(_))
        ));
    }

    #[test]
    fn test_validator_assignment() {
        let mut mgr = make_shard_manager(4, 8);
        register_validators(&mut mgr, 16, 1000);

        let assignment = mgr.assign_validators("vrf-seed-epoch-0").unwrap();

        // All 16 validators should be assigned
        assert_eq!(assignment.assignments.len(), 16);

        // Each shard should have 4 validators (16 / 4 shards)
        for i in 0..4 {
            let shard = mgr.get_shard(i).unwrap();
            assert_eq!(
                shard.validators.len(),
                4,
                "shard {} should have 4 validators",
                i
            );
        }

        // Each validator should know its shard
        for validator in mgr.all_validators().values() {
            assert!(validator.assigned_shard.is_some());
        }
    }

    #[test]
    fn test_insufficient_validators() {
        let mut mgr = make_shard_manager(4, 8);
        // Need at least 4 * 2 = 8 validators (min_validators_per_shard = 2)
        register_validators(&mut mgr, 4, 1000);

        let result = mgr.assign_validators("vrf-seed");
        assert!(matches!(
            result,
            Err(ShardingError::InsufficientValidators { .. })
        ));
    }

    #[test]
    fn test_shard_rotation() {
        let mut mgr = make_shard_manager(4, 8);
        register_validators(&mut mgr, 16, 1000);

        let assignment_1 = mgr.assign_validators("seed-epoch-0").unwrap();
        assert_eq!(mgr.current_epoch(), 0);

        let assignment_2 = mgr.rotate_shards("seed-epoch-1").unwrap();
        assert_eq!(mgr.current_epoch(), 1);

        // Assignments should differ with different seeds
        // (It is theoretically possible for them to be the same, but extremely unlikely)
        let same_count = assignment_1
            .assignments
            .iter()
            .filter(|(addr, shard1)| assignment_2.assignments.get(*addr) == Some(shard1))
            .count();

        // With 16 validators across 4 shards, random reassignment should change most assignments
        assert!(
            same_count < 16,
            "rotation should change at least some assignments"
        );
    }

    #[test]
    fn test_cross_shard_message_routing() {
        let mut mgr = make_shard_manager(4, 8);
        register_validators(&mut mgr, 16, 1000);
        mgr.assign_validators("seed").unwrap();

        // Set a state root for shard 0
        mgr.submit_shard_block(0, "state-root-0".to_string(), 5)
            .unwrap();

        // Send a message from shard 0 to shard 2
        let msg_id = mgr
            .send_cross_shard_message(0, 2, b"transfer-data".to_vec(), 10, 1000)
            .unwrap();

        assert!(!msg_id.is_empty());

        // Message should be pending for shard 2
        let pending = mgr.router().pending_for_shard(2);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].source_shard, 0);
        assert_eq!(pending[0].dest_shard, 2);

        // Deliver the message
        let delivered = mgr.deliver_cross_shard_message(&msg_id).unwrap();
        assert!(delivered.delivered);
        assert_eq!(delivered.source_shard, 0);
        assert_eq!(delivered.dest_shard, 2);
    }

    #[test]
    fn test_cross_shard_invalid_shard() {
        let mut mgr = make_shard_manager(4, 8);
        register_validators(&mut mgr, 16, 1000);
        mgr.assign_validators("seed").unwrap();

        let result = mgr.send_cross_shard_message(0, 99, vec![], 10, 1000);
        assert!(matches!(result, Err(ShardingError::ShardNotFound(99))));
    }

    #[test]
    fn test_beacon_chain_aggregation() {
        let mut mgr = make_shard_manager(4, 8);
        register_validators(&mut mgr, 16, 1000);
        mgr.assign_validators("seed").unwrap();

        // Submit shard blocks
        for i in 0..4 {
            mgr.submit_shard_block(i, format!("root-{}", i), 10)
                .unwrap();
        }

        // Aggregate state roots
        let global_root = mgr.aggregate_state();
        assert!(!global_root.is_empty());

        // Beacon chain should have all shard headers
        for i in 0..4 {
            assert!(mgr.beacon_chain().get_shard_header(i).is_some());
        }

        // Global state root should be in history
        assert_eq!(mgr.beacon_chain().state_root_history().len(), 1);
    }

    #[test]
    fn test_beacon_chain_deterministic() {
        // Two beacon chains with same shard headers should produce same global root
        let mut bc1 = BeaconChain::new();
        let mut bc2 = BeaconChain::new();

        for i in 0..4 {
            let root = format!("root-{}", i);
            bc1.submit_shard_header(i, root.clone(), 10, 50);
            bc2.submit_shard_header(i, root, 10, 50);
        }

        let root1 = bc1.aggregate_state_roots();
        let root2 = bc2.aggregate_state_roots();

        assert_eq!(root1, root2, "same inputs should produce same global root");
    }

    #[test]
    fn test_shard_block_submission() {
        let mut mgr = make_shard_manager(4, 8);

        let header = mgr
            .submit_shard_block(0, "new-root".to_string(), 15)
            .unwrap();

        assert_eq!(header.shard_id, 0);
        assert_eq!(header.state_root, "new-root");
        assert_eq!(header.transaction_count, 15);
        assert!(!header.header_hash.is_empty());

        let shard = mgr.get_shard(0).unwrap();
        assert_eq!(shard.block_height, 1);
        assert_eq!(shard.state_root, "new-root");
        assert_eq!(shard.transactions_processed, 15);
    }

    #[test]
    fn test_cross_shard_timeout() {
        let mut router = CrossShardRouter::new(10);

        router.send_message(0, 1, vec![1, 2, 3], "root".to_string(), 100, 1000);
        router.send_message(0, 2, vec![4, 5, 6], "root".to_string(), 100, 1000);

        // Before timeout: no messages timed out
        let timed_out = router.check_timeouts(105);
        assert!(timed_out.is_empty());

        // After timeout: both messages timed out
        let timed_out = router.check_timeouts(115);
        assert_eq!(timed_out.len(), 2);
    }

    #[test]
    fn test_shard_add_remove_validator() {
        let mut shard = Shard::new(0);

        shard.add_validator("val-a".to_string(), 4).unwrap();
        shard.add_validator("val-b".to_string(), 4).unwrap();
        assert_eq!(shard.validators.len(), 2);

        // Duplicate should fail
        let result = shard.add_validator("val-a".to_string(), 4);
        assert!(matches!(
            result,
            Err(ShardingError::ValidatorAlreadyAssigned(_))
        ));

        // Capacity exceeded
        shard.add_validator("val-c".to_string(), 4).unwrap();
        shard.add_validator("val-d".to_string(), 4).unwrap();
        let result = shard.add_validator("val-e".to_string(), 4);
        assert!(matches!(
            result,
            Err(ShardingError::ShardCapacityExceeded { .. })
        ));

        // Remove validator
        assert!(shard.remove_validator("val-b"));
        assert_eq!(shard.validators.len(), 3);
        assert!(!shard.remove_validator("val-nonexistent"));
    }

    #[test]
    fn test_vrf_assignment_deterministic() {
        // Same seed and validators should produce same assignment
        let mut mgr1 = make_shard_manager(4, 8);
        let mut mgr2 = make_shard_manager(4, 8);

        for i in 0..16 {
            mgr1.register_validator(format!("validator-{}", i), 1000)
                .unwrap();
            mgr2.register_validator(format!("validator-{}", i), 1000)
                .unwrap();
        }

        let a1 = mgr1.assign_validators("same-seed").unwrap();
        let a2 = mgr2.assign_validators("same-seed").unwrap();

        assert_eq!(a1.assignments, a2.assignments);
    }

    #[test]
    fn test_assignment_history() {
        let mut mgr = make_shard_manager(4, 8);
        register_validators(&mut mgr, 16, 1000);

        mgr.assign_validators("seed-0").unwrap();
        mgr.rotate_shards("seed-1").unwrap();
        mgr.rotate_shards("seed-2").unwrap();

        assert_eq!(mgr.assignment_history().len(), 3);
        assert_eq!(mgr.assignment_history()[0].epoch, 0);
        assert_eq!(mgr.assignment_history()[1].epoch, 1);
        assert_eq!(mgr.assignment_history()[2].epoch, 2);
    }
}
