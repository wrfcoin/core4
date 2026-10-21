use crate::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use crate::common::constants::DEFAULT_ORACLE_QUORUM;
use crate::consensus::proof_of_publishing::PublishingRole;
use chrono::Utc;
use log::{debug, info, warn};
use std::collections::HashMap;

/// Token Emission Manager handles the logic for distributing tokens
/// based on roles, region scarcity, accuracy, and reputation.
pub struct TokenEmissionManager {
    /// Base emission rate per block
    base_emission_rate: u64,

    /// Emission decay factor (percent of original emission that remains after each adjustment)
    emission_decay: f64,

    /// Block height at which last adjustment occurred
    last_adjustment_height: u64,

    /// Blocks between emission adjustments
    adjustment_interval: u64,

    /// Role-based reward percentages
    role_percentages: HashMap<PublishingRole, f64>,

    /// Region scarcity weights (higher weight = higher rewards)
    region_weights: HashMap<String, f64>,

    /// Maximum emission throttle (0.0-1.0, where 1.0 means full emission)
    emission_throttle: f64,

    /// Accuracy score multiplier for rewards
    accuracy_multiplier: f64,

    /// Reputation score multiplier for rewards
    reputation_multiplier: f64,

    /// Maximum emission per block after adjustments.
    max_emission_per_block: u64,
}

impl TokenEmissionManager {
    /// Create a new token emission manager
    pub fn new(base_emission_rate: u64, emission_decay: f64, adjustment_interval: u64) -> Self {
        // Default role distribution: 35% for analyzers, 30% for collectors,
        // 25% for keepers, and 10% for offsetters
        let mut role_percentages = HashMap::new();
        role_percentages.insert(PublishingRole::Analyzer, 0.35);
        role_percentages.insert(PublishingRole::Collector, 0.30);
        role_percentages.insert(PublishingRole::Keeper, 0.25);
        role_percentages.insert(PublishingRole::Offsetter, 0.10);

        Self {
            base_emission_rate,
            emission_decay,
            last_adjustment_height: 0,
            adjustment_interval,
            role_percentages,
            region_weights: HashMap::new(),
            emission_throttle: 1.0,
            accuracy_multiplier: 1.0,
            reputation_multiplier: 1.0,
            max_emission_per_block: base_emission_rate.saturating_mul(10),
        }
    }

    /// Calculate block reward based on current emission settings and height
    pub fn calculate_block_reward(&self, block_height: u64) -> u64 {
        // Calculate decay factor based on how many adjustments have occurred
        let adjustments = block_height / self.adjustment_interval;

        // Apply decay formula: base_rate * decay_factor^adjustments
        let decay_factor = self.emission_decay.powi(adjustments as i32);

        // Apply emission throttle
        let reward =
            (self.base_emission_rate as f64 * decay_factor * self.emission_throttle).round() as u64;

        debug!(
            "Block reward at height {}: {} tokens (decay: {:.4}, throttle: {:.2})",
            block_height, reward, decay_factor, self.emission_throttle
        );

        reward.min(self.max_emission_per_block)
    }

    /// Distribute rewards across different roles
    pub fn distribute_role_rewards(&self, total_reward: u64) -> HashMap<PublishingRole, u64> {
        let mut role_rewards = HashMap::new();

        // Calculate reward for each role based on percentages
        for (role, percentage) in &self.role_percentages {
            let role_reward = (total_reward as f64 * percentage).round() as u64;
            role_rewards.insert(*role, role_reward);
        }

        // Ensure we don't exceed total reward due to rounding
        let total_distributed: u64 = role_rewards.values().sum();
        if total_distributed > total_reward {
            // Adjust the largest allocation to make up the difference
            let role_to_adjust = role_rewards
                .iter()
                .max_by_key(|(_, &amount)| amount)
                .map(|(role, _)| *role);

            if let Some(role) = role_to_adjust {
                let difference = total_distributed - total_reward;
                if let Some(amount) = role_rewards.get_mut(&role) {
                    *amount -= difference;
                }
            }
        }

        role_rewards
    }

    /// Set custom role-based reward percentages
    pub fn set_role_percentages(&mut self, percentages: HashMap<PublishingRole, f64>) {
        // Validate that percentages add up to 1.0
        let total: f64 = percentages.values().sum();
        if (total - 1.0).abs() > 0.001 {
            warn!("Role percentages don't add up to 1.0 (sum: {total}), normalizing");
            // Normalize to ensure they add up to 1.0
            let mut normalized = HashMap::new();
            for (role, pct) in percentages {
                normalized.insert(role, pct / total);
            }
            self.role_percentages = normalized;
        } else {
            self.role_percentages = percentages;
        }
    }

    /// Set region scarcity weights
    pub fn set_region_weights(&mut self, weights: HashMap<String, f64>) {
        self.region_weights = weights;
    }

    /// Get region weight for a specific region
    pub fn get_region_weight(&self, region: &str) -> f64 {
        self.region_weights.get(region).copied().unwrap_or(1.0)
    }

    /// Set emission throttle (0.0-1.0)
    pub fn set_emission_throttle(&mut self, throttle: f64) {
        self.emission_throttle = throttle.max(0.0).min(1.0);
        info!("Emission throttle set to {:.2}", self.emission_throttle);
    }

    /// Set accuracy multiplier for rewards
    pub fn set_accuracy_multiplier(&mut self, multiplier: f64) {
        self.accuracy_multiplier = multiplier.max(0.0);
    }

    /// Set reputation multiplier for rewards
    pub fn set_reputation_multiplier(&mut self, multiplier: f64) {
        self.reputation_multiplier = multiplier.max(0.0);
    }

    /// Apply region scarcity, accuracy and reputation adjustments to a reward
    pub fn apply_adjustments(
        &self,
        base_reward: u64,
        region: Option<&str>,
        accuracy: Option<f64>,
        reputation: Option<f64>,
    ) -> u64 {
        let mut adjusted_reward = base_reward as f64;

        // Apply region weight if available
        if let Some(region_name) = region {
            let region_weight = self.get_region_weight(region_name);
            adjusted_reward *= region_weight;
        }

        // Apply accuracy adjustment if available
        if let Some(acc) = accuracy {
            // Accuracy below 0.5 reduces reward, above 0.5 increases it
            let accuracy_factor = 1.0 + (acc - 0.5) * self.accuracy_multiplier;
            adjusted_reward *= accuracy_factor.max(0.1); // Never below 10% of base
        }

        // Apply reputation adjustment if available
        if let Some(rep) = reputation {
            // Reputation below 0.5 reduces reward, above 0.5 increases it
            let reputation_factor = 1.0 + (rep - 0.5) * self.reputation_multiplier;
            adjusted_reward *= reputation_factor.max(0.1); // Never below 10% of base
        }

        adjusted_reward.round() as u64
    }

    /// Calculate a contribution reward gated by oracle quality and optional DAO emission override.
    pub fn calculate_contribution_reward(
        &self,
        block_height: u64,
        contribution_score: f64,
        oracle_quality: f64,
        dao_emission_rate: Option<u64>,
    ) -> u64 {
        let emission =
            dao_emission_rate.unwrap_or_else(|| self.calculate_block_reward(block_height));
        let normalized_score = contribution_score.max(0.0) / 10.0;
        let oracle_factor = oracle_quality
            .clamp(0.0, 1.0)
            .max(1.0 / DEFAULT_ORACLE_QUORUM as f64);
        let reward = (emission as f64 * normalized_score * oracle_factor).round() as u64;
        reward.min(self.max_emission_per_block)
    }

    pub fn calculate_forecast_skill_reward(
        &self,
        block_height: u64,
        forecast_skill: f64,
        oracle_quality: f64,
        dao_emission_rate: Option<u64>,
    ) -> u64 {
        let emission =
            dao_emission_rate.unwrap_or_else(|| self.calculate_block_reward(block_height));
        let skill_factor = forecast_skill.clamp(0.0, 1.0);
        let oracle_factor = oracle_quality
            .clamp(0.0, 1.0)
            .max(1.0 / DEFAULT_ORACLE_QUORUM as f64);
        let reward = (emission as f64 * skill_factor * oracle_factor).round() as u64;
        reward.min(self.max_emission_per_block)
    }

    pub fn set_max_emission_per_block(&mut self, cap: u64) {
        self.max_emission_per_block = cap.max(1);
    }

    /// Check if emission adjustment should occur at this height
    pub fn should_adjust_emission(&self, current_height: u64) -> bool {
        // Check if we've reached the next adjustment interval
        if current_height > 0 && current_height % self.adjustment_interval == 0 {
            let adjustments_occurred = current_height / self.adjustment_interval;
            let last_adjustments = self.last_adjustment_height / self.adjustment_interval;

            // Only adjust if we've reached a new adjustment point
            adjustments_occurred > last_adjustments
        } else {
            false
        }
    }

    /// Register that an adjustment occurred at given height
    pub fn register_adjustment(&mut self, height: u64) {
        self.last_adjustment_height = height;
    }

    /// Create a transaction rewarding a specific role
    pub fn create_role_reward_transaction(
        &self,
        role: PublishingRole,
        recipient: &str,
        amount: u64,
    ) -> Transaction {
        // Create a coinbase-like input for the reward
        let reward_input = TransactionInput {
            txid: [0; 32].into(),
            vout: 0xFFFFFFFF,
            signature: None,
            public_key: None,
            coinbase_data: Some(format!("Role reward for {}", role.to_str())),
        };

        // Create output for the recipient
        let reward_output = TransactionOutput {
            amount,
            address: recipient.to_string(),
            script: format!("OP_DUP OP_HASH160 {recipient} OP_EQUALVERIFY OP_CHECKSIG"),
            nft_data: None,
        };

        // Create and return the transaction
        let mut tx = Transaction::new(vec![reward_input], vec![reward_output]);
        tx.tx_type = TransactionType::DataAnalysis; // Reward bookkeeping tx type
        tx.timestamp = Utc::now();
        tx.data = Some(format!(
            "{{\"reward_type\":\"role_reward\",\"role\":\"{}\"}}",
            role.to_str()
        ));
        tx.fee = 0;
        tx.set_id();

        tx
    }
}

impl Default for TokenEmissionManager {
    fn default() -> Self {
        // Default settings:
        // - 100 tokens per block initially
        // - 0.99 decay factor (1% reduction per adjustment)
        // - adjustment every 210,000 blocks (approximately every 4 years at 10-minute blocks)
        Self::new(100, 0.99, 210_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager_with_interval(interval: u64) -> TokenEmissionManager {
        // decay=1.0 so rewards don't change between adjustments — isolates boundary logic
        TokenEmissionManager::new(1_000, 1.0, interval)
    }

    #[test]
    fn adjustment_count_at_height_zero() {
        let mgr = manager_with_interval(100);
        // height=0: 0/100 = 0 adjustments → full base reward
        let reward = mgr.calculate_block_reward(0);
        assert_eq!(reward, 1_000);
    }

    #[test]
    fn adjustment_count_just_before_first_boundary() {
        let mgr = manager_with_interval(100);
        // height=99: 99/100 = 0 adjustments → still full base reward
        let reward = mgr.calculate_block_reward(99);
        assert_eq!(reward, 1_000);
    }

    #[test]
    fn adjustment_count_at_first_boundary() {
        let mgr = manager_with_interval(100);
        // height=100: 100/100 = 1 adjustment; decay=1.0 → reward unchanged
        let reward = mgr.calculate_block_reward(100);
        assert_eq!(reward, 1_000);
    }

    #[test]
    fn adjustment_count_just_after_first_boundary() {
        let mgr = manager_with_interval(100);
        // height=101: 101/100 = 1 adjustment (same as height=100, not 2)
        let reward_at = mgr.calculate_block_reward(100);
        let reward_after = mgr.calculate_block_reward(101);
        assert_eq!(reward_at, reward_after, "reward must not change one block after boundary");
    }

    #[test]
    fn adjustment_count_at_second_boundary() {
        // Use decay=0.5 so we can tell apart 1 vs 2 adjustments
        let mgr = TokenEmissionManager::new(1_000, 0.5, 100);
        // height=100 → 1 adjustment → 1000 * 0.5^1 = 500
        // height=200 → 2 adjustments → 1000 * 0.5^2 = 250
        let reward_first = mgr.calculate_block_reward(100);
        let reward_second = mgr.calculate_block_reward(200);
        assert_eq!(reward_first, 500);
        assert_eq!(reward_second, 250);
    }
}
