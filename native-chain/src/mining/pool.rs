use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::hash::Hash;
type Timestamp = u64;
use crate::mining::reward::{MiningContribution, ContributionType};
use anyhow::Result;
use log::info;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sha2::Digest;
use serde::{Serialize, Deserialize};

/// Mining pool member status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberStatus {
    /// Active and in good standing
    Active,

    /// Pending approval
    Pending,

    /// Inactive
    Inactive,

    /// Suspended due to misbehavior
    Suspended,
}

/// Mining pool member
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMember {
    /// Member address
    pub address: Address,

    /// Member status
    pub status: MemberStatus,

    /// Reputation score
    pub reputation: u8,

    /// Computational power
    pub computational_power: u64,

    /// Contribution history
    pub contribution_history: Vec<MiningContribution>,

    /// Join timestamp
    pub join_timestamp: Timestamp,

    /// Last active timestamp
    pub last_active: Timestamp,

    /// Total rewards earned
    pub total_rewards: u64,
}

/// Mining task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is available
    Available,

    /// Task is assigned
    Assigned,

    /// Task is in progress
    InProgress,

    /// Task is completed
    Completed,

    /// Task has failed
    Failed,

    /// Task has expired
    Expired,
}

/// Mining task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningTask {
    /// Task ID
    pub id: String,

    /// Task type
    pub task_type: ContributionType,

    /// Task status
    pub status: TaskStatus,

    /// Task creator
    pub creator: Address,

    /// Task assignee
    pub assignee: Option<Address>,

    /// Task creation timestamp
    pub creation_timestamp: Timestamp,

    /// Task deadline
    pub deadline: Timestamp,

    /// Task completion timestamp
    pub completion_timestamp: Option<Timestamp>,

    /// Task reward
    pub reward: u64,

    /// Task data
    pub data: Vec<u8>,

    /// Task result
    pub result: Option<Vec<u8>>,

    /// Task hash (for verification)
    pub hash: Hash,
}

/// Mining pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Pool name
    pub name: String,

    /// Pool owner
    pub owner: Address,

    /// Minimum reputation required to join
    pub min_reputation: u8,

    /// Fee percentage
    pub fee_percentage: u8,

    /// Reward distribution frequency (in seconds)
    pub reward_distribution_frequency: u64,

    /// Minimum computational power required
    pub min_computational_power: u64,

    /// Task expiration time (in seconds)
    pub task_expiration_time: u64,
}

/// Mining pool
pub struct MiningPool {
    /// Pool configuration
    config: Arc<RwLock<PoolConfig>>,

    /// Pool members
    members: Arc<RwLock<HashMap<Address, PoolMember>>>,

    /// Mining tasks
    tasks: Arc<RwLock<HashMap<String, MiningTask>>>,

    /// Pool metrics
    metrics: Arc<RwLock<PoolMetrics>>,

    /// Per-miner difficulty state and threat tracking (Ref #960).
    miner_difficulty: Arc<RwLock<HashMap<Address, MinerDifficultyState>>>,
}

/// Mining pool metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolMetrics {
    /// Number of active members
    pub active_members: u64,

    /// Number of completed tasks
    pub completed_tasks: u64,

    /// Number of failed tasks
    pub failed_tasks: u64,

    /// Total rewards distributed
    pub total_rewards: u64,

    /// Total pool computational power
    pub total_computational_power: u64,

    /// Average task completion time
    pub avg_task_completion_time: f64,

    /// Pool success rate
    pub success_rate: f64,
}

/// A share submitted by a pool miner.
#[derive(Debug, Clone)]
pub struct MinerShare {
    /// Address of the miner who submitted the share.
    pub miner: Address,
    /// Blake3/SHA-256 hash of the partial work proof.
    pub share_hash: Hash,
    /// Nonce used to produce this share.
    pub nonce: u64,
    /// Timestamp of submission (unix seconds).
    pub submitted_at: u64,
}

/// Result of validating a share against the pool difficulty target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareValidationResult {
    /// Share meets the current pool difficulty target for this miner.
    Valid,
    /// Share does not meet the required difficulty target.
    BelowTarget,
    /// The miner is not a member of this pool or is not active.
    UnknownMiner,
}

/// Per-miner difficulty and threat tracking state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerDifficultyState {
    /// Current difficulty target for this miner (lower = harder).
    /// Stored as the number of leading zero bits required in the share hash.
    pub difficulty_bits: u32,
    /// Timestamps of recent share submissions (kept bounded).
    pub recent_submissions: Vec<u64>,
    /// Accumulated suspicion score from invalid shares (0 = clean).
    pub threat_score: u32,
    /// Total valid shares submitted.
    pub valid_shares: u64,
    /// Total invalid shares submitted.
    pub invalid_shares: u64,
}

impl Default for MinerDifficultyState {
    fn default() -> Self {
        Self {
            difficulty_bits: DEFAULT_POOL_DIFFICULTY_BITS,
            recent_submissions: Vec::new(),
            threat_score: 0,
            valid_shares: 0,
            invalid_shares: 0,
        }
    }
}

/// Default pool difficulty: 16 leading zero bits in the share hash.
const DEFAULT_POOL_DIFFICULTY_BITS: u32 = 16;

/// Minimum pool difficulty (easiest): 8 leading zero bits.
const MIN_POOL_DIFFICULTY_BITS: u32 = 8;

/// Maximum pool difficulty (hardest): 32 leading zero bits.
const MAX_POOL_DIFFICULTY_BITS: u32 = 32;

/// Target share submission rate: one share every 10 seconds.
const TARGET_SHARE_INTERVAL_SECS: u64 = 10;

/// How many recent submissions to keep for rate calculation.
const MAX_RECENT_SUBMISSIONS: usize = 64;

/// Threat score increment per invalid share.
const THREAT_SCORE_PER_INVALID: u32 = 5;

/// Threat score decay per valid share.
const THREAT_SCORE_DECAY_PER_VALID: u32 = 1;

/// Threshold above which a miner is considered suspicious.
pub const THREAT_SCORE_SUSPEND_THRESHOLD: u32 = 50;

impl MiningPool {
    /// Create a new mining pool
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            members: Arc::new(RwLock::new(HashMap::new())),
            tasks: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(PoolMetrics::default())),
            miner_difficulty: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get pool configuration
    pub async fn get_config(&self) -> PoolConfig {
        self.config.read().await.clone()
    }

    /// Update pool configuration
    pub async fn update_config(&self, config: PoolConfig) -> Result<()> {
        let mut pool_config = self.config.write().await;
        *pool_config = config;

        info!("Pool configuration updated: {}", pool_config.name);

        Ok(())
    }

    /// Add a member to the pool
    pub async fn add_member(&self, address: Address, computational_power: u64) -> Result<()> {
        let config = self.config.read().await;

        // Check if computational power meets minimum requirements
        if computational_power < config.min_computational_power {
            return Err(anyhow::anyhow!(
                "Computational power too low: {} (minimum: {})",
                computational_power,
                config.min_computational_power
            ));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Create new member
        let member = PoolMember {
            address,
            status: MemberStatus::Pending, // Pending approval
            reputation: 75, // Start with a neutral-positive reputation
            computational_power,
            contribution_history: Vec::new(),
            join_timestamp: timestamp,
            last_active: timestamp,
            total_rewards: 0,
        };

        // Add member to pool
        let mut members = self.members.write().await;
        members.insert(address, member);

        info!("New member added to pool: {}", hex::encode(address.as_bytes()));

        // Update metrics
        drop(members);
        drop(config);
        self.update_metrics().await?;

        Ok(())
    }

    /// Approve a pending member
    pub async fn approve_member(&self, address: Address) -> Result<()> {
        let mut members = self.members.write().await;

        let member = members.get_mut(&address).ok_or_else(|| {
            anyhow::anyhow!("Member not found: {}", hex::encode(address.as_bytes()))
        })?;

        if member.status != MemberStatus::Pending {
            return Err(anyhow::anyhow!("Member is not pending approval"));
        }

        // Update status
        member.status = MemberStatus::Active;

        info!("Member approved: {}", hex::encode(address.as_bytes()));

        // Update metrics
        drop(members); // Release lock before calling update_metrics
        self.update_metrics().await?;

        Ok(())
    }

    /// Remove a member from the pool
    pub async fn remove_member(&self, address: Address) -> Result<()> {
        let mut members = self.members.write().await;

        if !members.contains_key(&address) {
            return Err(anyhow::anyhow!("Member not found: {}", hex::encode(address.as_bytes())));
        }

        // Remove member
        members.remove(&address);

        info!("Member removed: {}", hex::encode(address.as_bytes()));

        // Update metrics
        drop(members); // Release lock before calling update_metrics
        self.update_metrics().await?;

        Ok(())
    }

    /// Create a new mining task
    pub async fn create_task(&self,
                           creator: Address,
                           task_type: ContributionType,
                           data: Vec<u8>,
                           reward: u64) -> Result<String> {
        // Verify creator is a member and is active
        let members = self.members.read().await;

        let member = members.get(&creator).ok_or_else(|| {
            anyhow::anyhow!("Creator is not a pool member: {}", hex::encode(creator.as_bytes()))
        })?;

        if member.status != MemberStatus::Active {
            return Err(anyhow::anyhow!("Creator is not an active member"));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let config = self.config.read().await;

        // Generate task ID
        let id = format!("task-{}-{}", timestamp, rand::random::<u32>());

        // Generate task hash
        let mut hasher = sha2::Sha256::new();
        hasher.update(id.as_bytes());
        hasher.update(&data);
        hasher.update(creator.as_bytes());
        let result: [u8; 32] = hasher.finalize().into();
        let hash = Hash::from(result);

        // Create task
        let task = MiningTask {
            id: id.clone(),
            task_type,
            status: TaskStatus::Available,
            creator,
            assignee: None,
            creation_timestamp: timestamp,
            deadline: timestamp + config.task_expiration_time,
            completion_timestamp: None,
            reward,
            data,
            result: None,
            hash,
        };

        // Add task
        let mut tasks = self.tasks.write().await;
        tasks.insert(id.clone(), task);

        info!("New mining task created: {}", id);

        Ok(id)
    }

    /// Assign a task to a member
    pub async fn assign_task(&self, task_id: &str, member_address: Address) -> Result<()> {
        // Verify member is active
        let members = self.members.read().await;

        let member = members.get(&member_address).ok_or_else(|| {
            anyhow::anyhow!("Member not found: {}", hex::encode(member_address.as_bytes()))
        })?;

        if member.status != MemberStatus::Active {
            return Err(anyhow::anyhow!("Member is not active"));
        }

        // Update task
        let mut tasks = self.tasks.write().await;

        let task = tasks.get_mut(task_id).ok_or_else(|| {
            anyhow::anyhow!("Task not found: {}", task_id)
        })?;

        if task.status != TaskStatus::Available {
            return Err(anyhow::anyhow!("Task is not available"));
        }

        // Assign task
        task.status = TaskStatus::Assigned;
        task.assignee = Some(member_address);

        info!("Task {} assigned to {}", task_id, hex::encode(member_address.as_bytes()));

        Ok(())
    }

    /// Complete a task
    pub async fn complete_task(&self,
                              task_id: &str,
                              member_address: Address,
                              result: Vec<u8>) -> Result<()> {
        // Verify task exists and capture needed data
        let (task_reward, task_hash, task_type) = {
            let mut tasks = self.tasks.write().await;

            let task = tasks.get_mut(task_id).ok_or_else(|| {
                anyhow::anyhow!("Task not found: {}", task_id)
            })?;

            // Verify member is assigned to task
            if task.assignee != Some(member_address) {
                return Err(anyhow::anyhow!("Member is not assigned to this task"));
            }

            if task.status != TaskStatus::Assigned && task.status != TaskStatus::InProgress {
                return Err(anyhow::anyhow!("Task is not assigned or in progress"));
            }

            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // Mark task as completed
            task.status = TaskStatus::Completed;
            task.completion_timestamp = Some(timestamp);
            task.result = Some(result);

            (task.reward, task.hash, task.task_type)
        };

        // Update member stats
        let mut members = self.members.write().await;
        if let Some(member) = members.get_mut(&member_address) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            member.last_active = timestamp;
            member.total_rewards += task_reward;

            // Record contribution
            let contribution = self.create_contribution_from_task_data(
                task_id, member_address, task_reward, task_hash, task_type
            ).await?;
            member.contribution_history.push(contribution);
        }

        info!("Task {} completed by {}", task_id, hex::encode(member_address.as_bytes()));

        // Update metrics
        drop(members); // Release lock
        self.update_metrics().await?;

        Ok(())
    }

    /// Create a mining contribution from task data (called after task lock is released)
    async fn create_contribution_from_task_data(
        &self,
        task_id: &str,
        member_address: Address,
        task_reward: u64,
        task_hash: Hash,
        task_type: ContributionType,
    ) -> Result<MiningContribution> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Create proof data
        let mut proof = Vec::new();
        proof.extend_from_slice(task_id.as_bytes());
        proof.extend_from_slice(task_hash.as_bytes());

        // Check result from tasks map
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(task_id) {
            if let Some(result) = &task.result {
                proof.extend_from_slice(result);
            }
        }

        // Create contribution
        let contribution = MiningContribution {
            contributor: member_address,
            contribution_type: task_type,
            timestamp,
            hash: task_hash,
            weight: task_reward, // Use reward as contribution weight
            proof,
            verifications: HashMap::new(), // To be filled by validators
            block_height: None, // To be filled when included in a block
            block_hash: None, // To be filled when included in a block
        };

        Ok(contribution)
    }

    /// Get available tasks
    pub async fn get_available_tasks(&self) -> Vec<MiningTask> {
        let tasks = self.tasks.read().await;

        tasks.values()
            .filter(|task| task.status == TaskStatus::Available)
            .cloned()
            .collect()
    }

    /// Get member tasks
    pub async fn get_member_tasks(&self, member_address: Address) -> Vec<MiningTask> {
        let tasks = self.tasks.read().await;

        tasks.values()
            .filter(|task| task.assignee == Some(member_address))
            .cloned()
            .collect()
    }

    /// Get pool members
    pub async fn get_members(&self) -> Vec<PoolMember> {
        let members = self.members.read().await;
        members.values().cloned().collect()
    }

    /// Get pool metrics
    pub async fn get_metrics(&self) -> PoolMetrics {
        self.metrics.read().await.clone()
    }

    /// Update pool metrics
    async fn update_metrics(&self) -> Result<()> {
        let mut metrics = PoolMetrics::default();

        // Update member metrics
        let members = self.members.read().await;
        metrics.active_members = members.values()
            .filter(|m| m.status == MemberStatus::Active)
            .count() as u64;

        metrics.total_computational_power = members.values()
            .filter(|m| m.status == MemberStatus::Active)
            .map(|m| m.computational_power)
            .sum();

        metrics.total_rewards = members.values()
            .map(|m| m.total_rewards)
            .sum();

        // Update task metrics
        let tasks = self.tasks.read().await;

        let completed_tasks: Vec<_> = tasks.values()
            .filter(|t| t.status == TaskStatus::Completed)
            .collect();

        metrics.completed_tasks = completed_tasks.len() as u64;

        metrics.failed_tasks = tasks.values()
            .filter(|t| t.status == TaskStatus::Failed)
            .count() as u64;

        // Calculate success rate
        let total_finished = metrics.completed_tasks + metrics.failed_tasks;
        if total_finished > 0 {
            metrics.success_rate = metrics.completed_tasks as f64 / total_finished as f64;
        }

        // Calculate average completion time
        if !completed_tasks.is_empty() {
            let total_time: u64 = completed_tasks.iter()
                .filter_map(|t| {
                    if let Some(completion) = t.completion_timestamp {
                        Some(completion - t.creation_timestamp)
                    } else {
                        None
                    }
                })
                .sum();

            metrics.avg_task_completion_time = total_time as f64 / completed_tasks.len() as f64;
        }

        // Update metrics
        *self.metrics.write().await = metrics;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Pool share validation, per-miner difficulty adjustment, threat scoring
    // (Ref #960)
    // -----------------------------------------------------------------------

    /// Validate a submitted share against the pool's per-miner difficulty target.
    ///
    /// Returns [`ShareValidationResult::Valid`] when the share hash has at least
    /// as many leading zero bits as the miner's current difficulty target.
    /// On a valid share the miner's difficulty is adjusted and threat score
    /// decays.  On an invalid share the threat score increases and the miner
    /// may be auto-suspended.
    pub async fn validate_share(&self, share: &MinerShare) -> Result<ShareValidationResult> {
        // Verify the miner is an active pool member.
        {
            let members = self.members.read().await;
            match members.get(&share.miner) {
                Some(m) if m.status == MemberStatus::Active => {},
                _ => return Ok(ShareValidationResult::UnknownMiner),
            }
        }

        let mut diff_states = self.miner_difficulty.write().await;
        let state = diff_states.entry(share.miner).or_insert_with(Default::default);

        let required_bits = state.difficulty_bits;
        let leading_zeros = Self::count_leading_zero_bits(&share.share_hash);

        if leading_zeros >= required_bits {
            // Valid share.
            state.valid_shares += 1;
            state.threat_score = state.threat_score.saturating_sub(THREAT_SCORE_DECAY_PER_VALID);

            // Record submission time for rate-based difficulty adjustment.
            state.recent_submissions.push(share.submitted_at);
            if state.recent_submissions.len() > MAX_RECENT_SUBMISSIONS {
                state.recent_submissions.remove(0);
            }

            // Adjust difficulty based on submission rate.
            Self::adjust_difficulty(state);

            info!(
                "Valid share from {}: leading_zeros={}, required={}",
                hex::encode(share.miner.as_bytes()),
                leading_zeros,
                required_bits,
            );

            Ok(ShareValidationResult::Valid)
        } else {
            // Invalid share.
            state.invalid_shares += 1;
            state.threat_score += THREAT_SCORE_PER_INVALID;

            info!(
                "Invalid share from {}: leading_zeros={}, required={}, threat_score={}",
                hex::encode(share.miner.as_bytes()),
                leading_zeros,
                required_bits,
                state.threat_score,
            );

            // Auto-suspend if threat score exceeds threshold.
            let should_suspend = state.threat_score >= THREAT_SCORE_SUSPEND_THRESHOLD;
            let miner_addr = share.miner;

            // Must drop diff_states before acquiring members lock.
            drop(diff_states);

            if should_suspend {
                let mut members = self.members.write().await;
                if let Some(member) = members.get_mut(&miner_addr) {
                    if member.status == MemberStatus::Active {
                        member.status = MemberStatus::Suspended;
                        info!(
                            "Miner {} auto-suspended: threat_score >= {}",
                            hex::encode(miner_addr.as_bytes()),
                            THREAT_SCORE_SUSPEND_THRESHOLD,
                        );
                    }
                }
            }

            Ok(ShareValidationResult::BelowTarget)
        }
    }

    /// Adjust a miner's difficulty target based on their recent submission rate.
    ///
    /// If the miner submits shares faster than `TARGET_SHARE_INTERVAL_SECS`,
    /// difficulty increases (more leading zero bits required).  If slower,
    /// difficulty decreases.  Clamped to `[MIN_POOL_DIFFICULTY_BITS, MAX_POOL_DIFFICULTY_BITS]`.
    fn adjust_difficulty(state: &mut MinerDifficultyState) {
        if state.recent_submissions.len() < 2 {
            return;
        }

        let window = &state.recent_submissions;
        let elapsed = window.last().unwrap_or(&0).saturating_sub(*window.first().unwrap_or(&0));
        let intervals = (window.len() - 1) as u64;

        if intervals == 0 || elapsed == 0 {
            return;
        }

        let avg_interval = elapsed / intervals;

        if avg_interval < TARGET_SHARE_INTERVAL_SECS / 2 {
            // Submitting too fast — increase difficulty.
            state.difficulty_bits = (state.difficulty_bits + 1).min(MAX_POOL_DIFFICULTY_BITS);
        } else if avg_interval > TARGET_SHARE_INTERVAL_SECS * 2 {
            // Submitting too slowly — decrease difficulty.
            state.difficulty_bits = state.difficulty_bits.saturating_sub(1).max(MIN_POOL_DIFFICULTY_BITS);
        }
    }

    /// Count the number of leading zero bits in a hash value.
    fn count_leading_zero_bits(hash: &Hash) -> u32 {
        let bytes = hash.as_bytes();
        let mut count = 0u32;
        for &byte in bytes {
            if byte == 0 {
                count += 8;
            } else {
                count += byte.leading_zeros();
                break;
            }
        }
        count
    }

    /// Get the current per-miner difficulty state for a given address.
    pub async fn get_miner_difficulty(&self, miner: &Address) -> Option<MinerDifficultyState> {
        let states = self.miner_difficulty.read().await;
        states.get(miner).cloned()
    }

    /// Get the threat score for a given miner.  Returns 0 for unknown miners.
    pub async fn get_threat_score(&self, miner: &Address) -> u32 {
        let states = self.miner_difficulty.read().await;
        states.get(miner).map_or(0, |s| s.threat_score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pool() -> MiningPool {
        MiningPool::new(PoolConfig {
            name: "test-pool".to_string(),
            owner: Address([1u8; 20]),
            min_reputation: 0,
            fee_percentage: 1,
            reward_distribution_frequency: 600,
            min_computational_power: 1,
            task_expiration_time: 3600,
        })
    }

    fn make_share(miner: Address, leading_zero_bytes: usize) -> MinerShare {
        let mut hash_bytes = [0xFFu8; 32];
        for i in 0..leading_zero_bytes.min(32) {
            hash_bytes[i] = 0x00;
        }
        MinerShare {
            miner,
            share_hash: Hash::from(hash_bytes),
            nonce: 42,
            submitted_at: 1000,
        }
    }

    #[test]
    fn test_count_leading_zero_bits_all_zeros() {
        let hash = Hash::from([0u8; 32]);
        assert_eq!(MiningPool::count_leading_zero_bits(&hash), 256);
    }

    #[test]
    fn test_count_leading_zero_bits_first_byte_nonzero() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0x0F; // 4 leading zero bits
        let hash = Hash::from(bytes);
        assert_eq!(MiningPool::count_leading_zero_bits(&hash), 4);
    }

    #[test]
    fn test_count_leading_zero_bits_second_byte() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0x00;
        bytes[1] = 0x01; // 8 + 7 = 15 leading zero bits
        let hash = Hash::from(bytes);
        assert_eq!(MiningPool::count_leading_zero_bits(&hash), 15);
    }

    #[tokio::test]
    async fn test_validate_share_unknown_miner() {
        let pool = test_pool();
        let unknown = Address([99u8; 20]);
        let share = make_share(unknown, 4);
        let result = pool.validate_share(&share).await.unwrap();
        assert_eq!(result, ShareValidationResult::UnknownMiner);
    }

    #[tokio::test]
    async fn test_validate_share_valid() {
        let pool = test_pool();
        let miner = Address([2u8; 20]);

        // Add miner as active member.
        pool.add_member(miner, 100).await.unwrap();
        pool.approve_member(miner).await.unwrap();

        // Share with 3 leading zero bytes = 24 leading zero bits >= default 16.
        let share = make_share(miner, 3);
        let result = pool.validate_share(&share).await.unwrap();
        assert_eq!(result, ShareValidationResult::Valid);

        let state = pool.get_miner_difficulty(&miner).await.unwrap();
        assert_eq!(state.valid_shares, 1);
        assert_eq!(state.invalid_shares, 0);
        assert_eq!(state.threat_score, 0);
    }

    #[tokio::test]
    async fn test_validate_share_below_target() {
        let pool = test_pool();
        let miner = Address([3u8; 20]);

        pool.add_member(miner, 100).await.unwrap();
        pool.approve_member(miner).await.unwrap();

        // Share with only 1 leading zero byte = 8 bits < default 16.
        let share = make_share(miner, 1);
        let result = pool.validate_share(&share).await.unwrap();
        assert_eq!(result, ShareValidationResult::BelowTarget);

        let state = pool.get_miner_difficulty(&miner).await.unwrap();
        assert_eq!(state.valid_shares, 0);
        assert_eq!(state.invalid_shares, 1);
        assert_eq!(state.threat_score, THREAT_SCORE_PER_INVALID);
    }

    #[tokio::test]
    async fn test_threat_score_accumulates_and_suspends() {
        let pool = test_pool();
        let miner = Address([4u8; 20]);

        pool.add_member(miner, 100).await.unwrap();
        pool.approve_member(miner).await.unwrap();

        // Submit enough invalid shares to exceed the suspend threshold.
        let needed = (THREAT_SCORE_SUSPEND_THRESHOLD / THREAT_SCORE_PER_INVALID) + 1;
        for _ in 0..needed {
            // Share hash with too few leading zeros to pass validation.
            let mut bad_hash = [0xFFu8; 32];
            bad_hash[0] = 0x01; // only 7 leading zero bits < 16
            let bad_share = MinerShare {
                miner,
                share_hash: Hash::from(bad_hash),
                nonce: 1,
                submitted_at: 1000,
            };
            let _ = pool.validate_share(&bad_share).await.unwrap();
        }

        // Verify miner is suspended.
        let members = pool.members.read().await;
        let member = members.get(&miner).unwrap();
        assert_eq!(member.status, MemberStatus::Suspended);
    }

    #[test]
    fn test_difficulty_adjustment_faster_increases() {
        let mut state = MinerDifficultyState::default();
        // Simulate fast submissions: one every 2 seconds over 10 submissions.
        for i in 0..10u64 {
            state.recent_submissions.push(100 + i * 2);
        }
        let before = state.difficulty_bits;
        MiningPool::adjust_difficulty(&mut state);
        assert!(
            state.difficulty_bits > before,
            "difficulty should increase when shares arrive faster than target"
        );
    }

    #[test]
    fn test_difficulty_adjustment_slower_decreases() {
        let mut state = MinerDifficultyState::default();
        // Simulate slow submissions: one every 30 seconds over 10 submissions.
        for i in 0..10u64 {
            state.recent_submissions.push(100 + i * 30);
        }
        let before = state.difficulty_bits;
        MiningPool::adjust_difficulty(&mut state);
        assert!(
            state.difficulty_bits < before,
            "difficulty should decrease when shares arrive slower than target"
        );
    }

    #[test]
    fn test_difficulty_clamped_to_bounds() {
        // Test max bound.
        let mut state = MinerDifficultyState {
            difficulty_bits: MAX_POOL_DIFFICULTY_BITS,
            ..Default::default()
        };
        for i in 0..10u64 {
            state.recent_submissions.push(100 + i * 1); // Very fast
        }
        MiningPool::adjust_difficulty(&mut state);
        assert_eq!(state.difficulty_bits, MAX_POOL_DIFFICULTY_BITS);

        // Test min bound.
        let mut state = MinerDifficultyState {
            difficulty_bits: MIN_POOL_DIFFICULTY_BITS,
            ..Default::default()
        };
        for i in 0..10u64 {
            state.recent_submissions.push(100 + i * 60); // Very slow
        }
        MiningPool::adjust_difficulty(&mut state);
        assert_eq!(state.difficulty_bits, MIN_POOL_DIFFICULTY_BITS);
    }
}
