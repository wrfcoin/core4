use crate::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use crate::common::constants::DEFAULT_ORACLE_QUORUM;
use crate::consensus::proof_of_publishing::{
    MAX_BLOCK_EMISSION, PublishingRole, adjusted_block_reward,
};
use crate::rewards::config::RewardConfig;
use chrono::Utc;
use log::{debug, info, warn};
use std::collections::BTreeMap;

/// Token Emission Manager handles the logic for distributing tokens
/// based on roles, region scarcity, accuracy, and reputation.
pub struct TokenEmissionManager {
    /// Shared reward configuration (role percentages, region weights, multipliers).
    pub(crate) reward_config: RewardConfig,

    /// Emission throttle in basis points (0-10_000, where 10_000 = 100%).
    /// Used for deterministic integer arithmetic in `effective_block_emission`.
    emission_throttle_bps: u32,

    /// Maximum emission per block after adjustments.
    max_emission_per_block: u64,
}

impl TokenEmissionManager {
    /// Derive the effective reward budget for a reward path from the consensus
    /// schedule, then optionally clamp it downward through governance.
    pub fn effective_block_emission(
        &self,
        consensus_reward: u64,
        dao_emission_rate: Option<u64>,
    ) -> u64 {
        let clamped_consensus = dao_emission_rate
            .map(|rate| consensus_reward.min(rate))
            .unwrap_or(consensus_reward);
        let capped = clamped_consensus.min(self.max_emission_per_block);
        // Integer BPS arithmetic: avoids f64 non-determinism across CPU architectures.
        let reward = (capped as u128 * self.emission_throttle_bps as u128 / 10_000) as u64;
        let reward = reward.min(capped);

        debug!(
            "Effective reward budget: consensus={}, governance={:?}, cap={}, throttle_bps={}, effective={}",
            consensus_reward,
            dao_emission_rate,
            self.max_emission_per_block,
            self.emission_throttle_bps,
            reward
        );

        reward
    }

    /// Distribute rewards across different roles using deterministic u64 integer
    /// arithmetic. Uses BTreeMap for deterministic iteration order and pure
    /// integer math (numerator/denominator) to avoid f64 non-determinism.
    ///
    /// The last role in sorted BTreeMap order absorbs any rounding remainder so
    /// the distribution sums exactly to `total_reward`.
    pub fn distribute_role_rewards(&self, total_reward: u64) -> BTreeMap<PublishingRole, u64> {
        let mut role_rewards = BTreeMap::new();
        let ppm_map = &self.reward_config.role_percentages_ppm;

        if ppm_map.is_empty() {
            return role_rewards;
        }

        let ppm_total: u64 = ppm_map.values().map(|&p| p as u64).sum();

        let mut distributed: u64 = 0;
        let last_idx = ppm_map.len() - 1;

        for (i, (&role, &ppm)) in ppm_map.iter().enumerate() {
            if i == last_idx {
                // Last role absorbs remainder for exact sum.
                role_rewards.insert(role, total_reward.saturating_sub(distributed));
            } else {
                let share = total_reward
                    .checked_mul(ppm as u64)
                    .map(|v| v / ppm_total)
                    .unwrap_or(0);
                role_rewards.insert(role, share);
                distributed = distributed.saturating_add(share);
            }
        }

        role_rewards
    }

    /// Set custom role-based reward percentages
    pub fn set_role_percentages(&mut self, percentages: BTreeMap<PublishingRole, f64>) {
        if let Err(e) = self.reward_config.set_role_percentages(percentages) {
            warn!("set_role_percentages failed: {e}");
        }
    }

    /// Set region scarcity weights
    pub fn set_region_weights(&mut self, weights: rustc_hash::FxHashMap<String, f64>) {
        self.reward_config.set_region_weights(weights);
    }

    /// Get region weight for a specific region
    pub fn get_region_weight(&self, region: &str) -> f64 {
        self.reward_config.region_weight(region)
    }

    /// Set emission throttle (0.0-1.0).
    /// Converts to BPS internally for deterministic computation.
    pub fn set_emission_throttle(&mut self, throttle: f64) {
        let clamped = throttle.clamp(0.0, 1.0);
        self.emission_throttle_bps = (clamped * 10_000.0).round() as u32;
        info!("Emission throttle set to {:.2} ({} BPS)", clamped, self.emission_throttle_bps);
    }

    /// Set accuracy multiplier for rewards
    pub fn set_accuracy_multiplier(&mut self, multiplier: f64) {
        self.reward_config
            .set_accuracy_multiplier(multiplier.max(0.0));
    }

    /// Set reputation multiplier for rewards
    pub fn set_reputation_multiplier(&mut self, multiplier: f64) {
        self.reward_config
            .set_reputation_multiplier(multiplier.max(0.0));
    }

    /// Apply region scarcity, accuracy and reputation adjustments to a reward.
    ///
    /// `None` for accuracy or reputation is treated as 0.5 (neutral).
    pub fn apply_adjustments(
        &self,
        base_reward: u64,
        region: Option<&str>,
        accuracy: Option<f64>,
        reputation: Option<f64>,
    ) -> u64 {
        self.reward_config.apply_adjustments(
            base_reward,
            region,
            accuracy.unwrap_or(0.5),
            reputation.unwrap_or(0.5),
        )
    }

    /// Calculate a contribution reward gated by oracle quality and optional DAO emission override.
    ///
    /// All f64 inputs are converted to BPS at the boundary; the hot path is pure integer math.
    pub fn calculate_contribution_reward(
        &self,
        consensus_reward: u64,
        contribution_score: f64,
        oracle_quality: f64,
        dao_emission_rate: Option<u64>,
    ) -> u64 {
        let emission = self.effective_block_emission(consensus_reward, dao_emission_rate);
        // Convert at boundary: score 0-10 → normalized 0-1 → BPS 0-10_000.
        // Truncate (not round): 1-ULP f64 differences must not cross a rounding
        // boundary and produce divergent BPS on x86 vs ARM (#1873 / #1849).
        let score_bps = ((contribution_score.max(0.0) / 10.0) * 10_000.0) as u64;
        let score_bps = score_bps.min(10_000);
        // Oracle floor: 1/quorum, stored as BPS integer.
        let oracle_floor_bps = 10_000_u64 / DEFAULT_ORACLE_QUORUM as u64;
        let oracle_bps = (oracle_quality.clamp(0.0, 1.0) * 10_000.0) as u64;
        let oracle_bps = oracle_bps.max(oracle_floor_bps);
        // Single u128 multiply then two divides avoids double truncation loss.
        let reward =
            (emission as u128 * score_bps as u128 * oracle_bps as u128 / 100_000_000) as u64;
        reward.min(emission)
    }

    /// Calculate a forecast-skill reward gated by oracle quality and optional DAO emission override.
    ///
    /// All f64 inputs are converted to BPS at the boundary; the hot path is pure integer math.
    pub fn calculate_forecast_skill_reward(
        &self,
        consensus_reward: u64,
        forecast_skill: f64,
        oracle_quality: f64,
        dao_emission_rate: Option<u64>,
    ) -> u64 {
        let emission = self.effective_block_emission(consensus_reward, dao_emission_rate);
        // Convert at boundary: skill 0-1 → BPS 0-10_000.
        // Truncate (not round) — same policy as calculate_contribution_reward (#1873).
        let skill_bps = (forecast_skill.clamp(0.0, 1.0) * 10_000.0) as u64;
        let oracle_floor_bps = 10_000_u64 / DEFAULT_ORACLE_QUORUM as u64;
        let oracle_bps = (oracle_quality.clamp(0.0, 1.0) * 10_000.0) as u64;
        let oracle_bps = oracle_bps.max(oracle_floor_bps);
        let reward =
            (emission as u128 * skill_bps as u128 * oracle_bps as u128 / 100_000_000) as u64;
        reward.min(emission)
    }

    pub fn set_max_emission_per_block(&mut self, cap: u64) {
        self.max_emission_per_block = cap.max(1);
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
        tx.tx_type = TransactionType::Reward;
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
        Self {
            reward_config: RewardConfig::default(),
            emission_throttle_bps: 10_000,
            max_emission_per_block: MAX_BLOCK_EMISSION.min(u64::MAX as u128) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reward_transaction_uses_reward_type() {
        let manager = TokenEmissionManager::default();
        let tx =
            manager.create_role_reward_transaction(PublishingRole::Collector, "wrf-test-addr", 500);
        assert_eq!(tx.tx_type, TransactionType::Reward);
        assert_eq!(tx.fee, 0);
        assert_eq!(tx.outputs[0].amount, 500);
    }

    #[test]
    fn governance_emission_rate_only_clamps_downward() {
        let manager = TokenEmissionManager::default();
        assert_eq!(manager.effective_block_emission(50, Some(100)), 50);
        assert_eq!(manager.effective_block_emission(50, Some(12)), 12);
    }

    #[test]
    fn contribution_reward_uses_adjusted_consensus_schedule() {
        let manager = TokenEmissionManager::default();
        let height = 25_000;
        let consensus_reward = adjusted_block_reward(height);
        // 8e10 * 600 / 14_400 = 3_333_333_333 base units (33.33… WRF). Asserted
        // a bare `2` before #2485 rescaled the reward out of coin-denomination,
        // then 208_333_333 before #2492 raised the genesis reward to 800 WRF.
        assert_eq!(consensus_reward, 3_333_333_333);
        assert_eq!(
            manager.calculate_contribution_reward(consensus_reward, 10.0, 1.0, None),
            3_333_333_333
        );
    }

    #[test]
    fn forecast_reward_uses_adjusted_consensus_schedule() {
        let manager = TokenEmissionManager::default();
        let height = 210_000;
        let consensus_reward = adjusted_block_reward(height);
        assert_eq!(
            manager.calculate_forecast_skill_reward(consensus_reward, 1.0, 1.0, None),
            consensus_reward
        );
    }

    #[test]
    fn emission_throttle_bps_conversion() {
        let mut manager = TokenEmissionManager::default();
        // throttle 0.5 → 5000 BPS
        manager.set_emission_throttle(0.5);
        assert_eq!(manager.emission_throttle_bps, 5000);
        assert_eq!(manager.effective_block_emission(1_000, None), 500);
        // throttle 1.0 → 10_000 BPS (full emission)
        manager.set_emission_throttle(1.0);
        assert_eq!(manager.emission_throttle_bps, 10_000);
        assert_eq!(manager.effective_block_emission(1_000, None), 1_000);
        // throttle 0.0 → 0 BPS (no emission)
        manager.set_emission_throttle(0.0);
        assert_eq!(manager.emission_throttle_bps, 0);
        assert_eq!(manager.effective_block_emission(1_000, None), 0);
    }

    #[test]
    fn effective_block_emission_integer_determinism() {
        let manager = TokenEmissionManager::default();
        // Same inputs must always produce same output (integer arithmetic guarantees this).
        let r1 = manager.effective_block_emission(100_000, Some(80_000));
        let r2 = manager.effective_block_emission(100_000, Some(80_000));
        assert_eq!(r1, r2);
        assert_eq!(r1, 80_000); // throttle=1.0, governance clamps to 80_000
    }

    /// #2969 option (a), owner-ratified: the default governance emission_rate
    /// is a denominated 100 WRF, and when a caller passes it this manager's
    /// budget is min(consensus, 100 WRF) — downward-only semantics are designed
    /// and kept. Asserting a flat 100 WRF regardless of the consensus input
    /// would assert the rejected option (b); the low-consensus branch below is
    /// what forbids that.
    ///
    /// #2969 option (c), owner-ruled 2026-09-03: the PRODUCER no longer passes
    /// this rate — the per-decision budget is `contribution_reward_allowance`
    /// and the rate is retired as a budget knob (see
    /// `emission_rate_no_longer_sizes_a_decision_2969c`). This test pins the
    /// seed's denomination and the manager's clamp contract, which any future
    /// caller of the `dao_emission_rate` parameter still inherits.
    #[test]
    fn governance_default_emission_rate_is_a_denominated_100_wrf_budget_2969() {
        let rate = crate::blockchain::state::GovernanceParameters::default().emission_rate;
        let expected_rate = 100 * crate::common::constants::WRF_UNITS_PER_COIN;
        assert_eq!(
            rate, expected_rate,
            "default emission_rate must be the denominated product 100 * WRF_UNITS_PER_COIN, \
             not a bare integer (#2969)"
        );

        let manager = TokenEmissionManager::default();
        // Early-era: E(t) above the rate — governance clamps the budget down
        // to exactly 100 WRF.
        let high_consensus = 4 * expected_rate;
        assert_eq!(
            manager.effective_block_emission(high_consensus, Some(rate)),
            expected_rate
        );
        // Decayed-era: E(t) below the rate — the schedule bounds the budget;
        // the rate does NOT grant above it (downward-only, per the :36 clamp
        // and its proptest invariant).
        let low_consensus = expected_rate / 3;
        assert_eq!(
            manager.effective_block_emission(low_consensus, Some(rate)),
            low_consensus
        );
        // Through the production reward fn: a max-score, max-quality decision
        // credits the full budget at this seam (multipliers compose later).
        assert_eq!(
            manager.calculate_contribution_reward(high_consensus, 10.0, 1.0, Some(rate)),
            expected_rate
        );
    }

    #[test]
    fn contribution_reward_integer_math() {
        let manager = TokenEmissionManager::default();
        // score=10.0 (max), oracle=1.0: full reward
        assert_eq!(
            manager.calculate_contribution_reward(10_000, 10.0, 1.0, None),
            10_000
        );
        // score=5.0, oracle=1.0: half reward
        assert_eq!(
            manager.calculate_contribution_reward(10_000, 5.0, 1.0, None),
            5_000
        );
        // score=0.0: zero reward
        assert_eq!(
            manager.calculate_contribution_reward(10_000, 0.0, 1.0, None),
            0
        );
    }

    #[test]
    fn forecast_reward_integer_math() {
        let manager = TokenEmissionManager::default();
        // skill=1.0, oracle=1.0: full reward
        assert_eq!(
            manager.calculate_forecast_skill_reward(10_000, 1.0, 1.0, None),
            10_000
        );
        // skill=0.5, oracle=1.0: half reward
        assert_eq!(
            manager.calculate_forecast_skill_reward(10_000, 0.5, 1.0, None),
            5_000
        );
        // skill=0.0: reward equals floor (oracle_floor / 10_000 * emission = 0 for small values)
        assert_eq!(
            manager.calculate_forecast_skill_reward(10_000, 0.0, 1.0, None),
            0
        );
    }

    #[test]
    fn distribute_role_rewards_breaks_ties_deterministically() {
        let mut manager = TokenEmissionManager::default();
        let mut percentages = BTreeMap::new();
        percentages.insert(PublishingRole::Collector, 0.26);
        percentages.insert(PublishingRole::Analyzer, 0.26);
        percentages.insert(PublishingRole::Keeper, 0.24);
        percentages.insert(PublishingRole::Offsetter, 0.24);
        manager.set_role_percentages(percentages);

        let rewards = manager.distribute_role_rewards(3);

        // With BTreeMap iteration order (Collector, Analyzer, Keeper, Offsetter
        // sorted by Ord derive), integer division assigns shares and the last
        // role absorbs the remainder. Sum must be exact.
        assert_eq!(rewards.values().sum::<u64>(), 3);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn effective_block_emission_never_exceeds_consensus_or_caps(
            consensus_reward in 0u64..1_000_000,
            max_emission_per_block in 1u64..1_000_000,
            dao_emission_rate in proptest::option::of(0u64..1_000_000),
        ) {
            let mut manager = TokenEmissionManager::default();
            manager.set_max_emission_per_block(max_emission_per_block);
            let reward = manager.effective_block_emission(consensus_reward, dao_emission_rate);
            prop_assert!(
                reward <= consensus_reward,
                "Reward {} exceeded consensus reward {}",
                reward, consensus_reward
            );
            prop_assert!(
                reward <= max_emission_per_block,
                "Reward {} exceeded manager cap {}",
                reward, max_emission_per_block
            );
            if let Some(rate) = dao_emission_rate {
                prop_assert!(
                    reward <= rate,
                    "Reward {} exceeded governance clamp {}",
                    reward, rate
                );
            }
        }
    }
}
