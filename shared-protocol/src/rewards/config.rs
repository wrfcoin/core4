use anyhow::{Result, anyhow};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wrfcoin_reputation_engine::{ReputationScore, SCORE_SCALE};

use crate::consensus::capability::CapabilityMultiplierPolicy;
use crate::consensus::data_desert::DataDesertPolicy;
use crate::consensus::hyper_local::HyperLocalPolicy;
use crate::consensus::proof_of_publishing::PublishingRole;

/// Parts-per-million scale: 1_000_000 = 100%.
pub const PPM_SCALE: u32 = 1_000_000;
/// Basis-points scale: 10_000 = 1.0x (for multipliers and weights).
pub const BPS_SCALE: u32 = 10_000;

/// Unified reward configuration — single source of truth for role percentages,
/// region weights, and quality multipliers used across the reward pipeline.
///
/// All fields use deterministic integer representations:
/// - Role percentages: parts-per-million (PPM), must sum to 1_000_000
/// - Region weights: basis points (BPS), where 10_000 = 1.0x
/// - Multipliers: basis points (BPS), where 10_000 = 1.0x
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardConfig {
    /// Percentage of block reward allocated to each publishing role.
    /// Values are in parts-per-million (PPM) and must sum to 1_000_000.
    /// Uses BTreeMap for deterministic iteration order in consensus-critical paths.
    pub role_percentages_ppm: BTreeMap<PublishingRole, u32>,

    /// Weight multiplier per geographic region in basis points (10_000 = 1.0x).
    /// Missing regions default to 10_000 (1.0x).
    pub region_weights_bps: FxHashMap<String, u32>,

    /// How strongly accuracy deviations from 0.5 affect rewards, in BPS (10_000 = 1.0x).
    pub accuracy_multiplier_bps: u32,

    /// How strongly reputation deviations from 0.5 affect rewards, in BPS (10_000 = 1.0x).
    pub reputation_multiplier_bps: u32,

    /// Policy for data-desert reward multipliers.
    pub data_desert_policy: DataDesertPolicy,

    /// Policy for hyper-local accuracy reward multipliers.
    pub hyper_local_policy: HyperLocalPolicy,

    /// Policy for contributor hardware capability scoring (#735).
    ///
    /// Default is **disabled** — all contributors get the `1.0×` baseline
    /// until governance explicitly activates the multiplier via a DAO
    /// proposal. `#[serde(default)]` lets older reward configs on disk
    /// deserialize cleanly into the new layout.
    #[serde(default)]
    pub capability_policy: CapabilityMultiplierPolicy,
}

impl Default for RewardConfig {
    fn default() -> Self {
        let mut role_percentages_ppm = BTreeMap::new();
        role_percentages_ppm.insert(PublishingRole::Analyzer, 350_000);
        role_percentages_ppm.insert(PublishingRole::Collector, 300_000);
        role_percentages_ppm.insert(PublishingRole::Keeper, 250_000);
        role_percentages_ppm.insert(PublishingRole::Offsetter, 100_000);

        Self {
            role_percentages_ppm,
            region_weights_bps: FxHashMap::default(),
            accuracy_multiplier_bps: BPS_SCALE,
            reputation_multiplier_bps: BPS_SCALE,
            data_desert_policy: DataDesertPolicy::default(),
            hyper_local_policy: HyperLocalPolicy::default(),
            capability_policy: CapabilityMultiplierPolicy::default(),
        }
    }
}

impl RewardConfig {
    /// Set role percentages from f64 fractions (0.0-1.0 each, sum ≈ 1.0).
    /// Converts to PPM internally. Returns error if empty or sum is not ~1.0.
    pub fn set_role_percentages(
        &mut self,
        percentages: BTreeMap<PublishingRole, f64>,
    ) -> Result<()> {
        if percentages.is_empty() {
            return Err(anyhow!("role percentages cannot be empty"));
        }
        let sum: f64 = percentages.values().sum();
        if (sum - 1.0).abs() > 0.001 {
            return Err(anyhow!("role percentages must sum to 1.0 (got {sum:.6})"));
        }
        // Convert each f64 fraction to PPM. If the sum is slightly off 1.0,
        // normalize first so the PPM values sum exactly.
        let entries: Vec<(PublishingRole, f64)> = if (sum - 1.0).abs() > f64::EPSILON {
            percentages
                .into_iter()
                .map(|(role, pct)| (role, pct / sum))
                .collect()
        } else {
            percentages.into_iter().collect()
        };
        let mut ppm_map = BTreeMap::new();
        let mut ppm_total: u32 = 0;
        let last_idx = entries.len() - 1;
        for (i, (role, pct)) in entries.iter().enumerate() {
            if i == last_idx {
                // Last role absorbs rounding remainder.
                ppm_map.insert(*role, PPM_SCALE - ppm_total);
            } else {
                let ppm = (pct * PPM_SCALE as f64).round() as u32;
                ppm_map.insert(*role, ppm);
                ppm_total += ppm;
            }
        }
        self.role_percentages_ppm = ppm_map;
        Ok(())
    }

    /// Set role percentages directly in PPM. Must sum to 1_000_000.
    pub fn set_role_percentages_ppm(
        &mut self,
        percentages: BTreeMap<PublishingRole, u32>,
    ) -> Result<()> {
        if percentages.is_empty() {
            return Err(anyhow!("role percentages cannot be empty"));
        }
        let sum: u32 = percentages.values().sum();
        if sum != PPM_SCALE {
            return Err(anyhow!(
                "role percentages PPM must sum to {PPM_SCALE} (got {sum})"
            ));
        }
        self.role_percentages_ppm = percentages;
        Ok(())
    }

    /// Replace the region weight table from f64 values (1.0 = neutral).
    /// Converts to BPS internally.
    pub fn set_region_weights(&mut self, weights: FxHashMap<String, f64>) {
        self.region_weights_bps = weights
            .into_iter()
            .map(|(k, v)| (k, (v.max(0.0) * BPS_SCALE as f64).round() as u32))
            .collect();
    }

    /// Set the accuracy multiplier from f64 (1.0 = neutral). Converts to BPS.
    pub fn set_accuracy_multiplier(&mut self, multiplier: f64) {
        self.accuracy_multiplier_bps = (multiplier.max(0.0) * BPS_SCALE as f64).round() as u32;
    }

    /// Set the reputation multiplier from f64 (1.0 = neutral). Converts to BPS.
    pub fn set_reputation_multiplier(&mut self, multiplier: f64) {
        self.reputation_multiplier_bps = (multiplier.max(0.0) * BPS_SCALE as f64).round() as u32;
    }

    /// Look up the BPS weight for a region, defaulting to 10_000 (1.0x).
    pub fn region_weight_bps(&self, region: &str) -> u32 {
        self.region_weights_bps
            .get(region)
            .copied()
            .unwrap_or(BPS_SCALE)
    }

    /// Look up the weight for a region as f64 (for backward-compatible APIs).
    pub fn region_weight(&self, region: &str) -> f64 {
        self.region_weight_bps(region) as f64 / BPS_SCALE as f64
    }

    /// Apply region, accuracy, and reputation adjustments to a base reward amount.
    ///
    /// Pure integer arithmetic using u128 intermediates.
    ///
    /// - `accuracy_factor_bps = (10_000 + (accuracy_bps - 5_000) * multiplier_bps / 10_000).max(1_000)`
    /// - `reputation_factor_bps = (10_000 + (reputation_bps - 5_000) * multiplier_bps / 10_000).max(1_000)`
    /// - Final reward = `base * rw_bps * accuracy_factor_bps * reputation_factor_bps / 10_000^3`
    pub fn apply_adjustments(
        &self,
        base: u64,
        region: Option<&str>,
        accuracy: f64,
        reputation: f64,
    ) -> u64 {
        let rw_bps = region.map_or(BPS_SCALE as u64, |r| self.region_weight_bps(r) as u64);

        // Convert f64 inputs to BPS at the boundary (single multiply, deterministic).
        let accuracy_bps = (accuracy.clamp(0.0, 1.0) * BPS_SCALE as f64).round() as i64;
        let reputation_bps = (reputation.clamp(0.0, 1.0) * BPS_SCALE as f64).round() as i64;

        let accuracy_factor = (BPS_SCALE as i64
            + (accuracy_bps - 5_000) * self.accuracy_multiplier_bps as i64 / BPS_SCALE as i64)
            .max(1_000) as u64;
        let reputation_factor = (BPS_SCALE as i64
            + (reputation_bps - 5_000) * self.reputation_multiplier_bps as i64 / BPS_SCALE as i64)
            .max(1_000) as u64;

        // u128 intermediates to avoid overflow.
        // Divide by BPS_SCALE^3 because we have three BPS factors.
        let scale_cubed = (BPS_SCALE as u128).pow(3);
        let result = base as u128 * rw_bps as u128 * accuracy_factor as u128
            * reputation_factor as u128
            / scale_cubed;
        result as u64
    }

    /// Apply region, accuracy, and reputation adjustments using a deterministic u64 reputation score.
    ///
    /// Pure integer arithmetic throughout:
    ///   - Region and accuracy factors use BPS integer math
    ///   - Reputation uses `ReputationScore::bonus_multiplier_u64()` (range [SCORE_SCALE, 2*SCORE_SCALE])
    pub fn apply_adjustments_with_score(
        &self,
        base: u64,
        region: Option<&str>,
        accuracy: f64,
        reputation: &ReputationScore,
    ) -> u64 {
        let rw_bps = region.map_or(BPS_SCALE as u64, |r| self.region_weight_bps(r) as u64);

        let accuracy_bps = (accuracy.clamp(0.0, 1.0) * BPS_SCALE as f64).round() as i64;
        let accuracy_factor = (BPS_SCALE as i64
            + (accuracy_bps - 5_000) * self.accuracy_multiplier_bps as i64 / BPS_SCALE as i64)
            .max(1_000) as u64;

        // base_adjusted = base * rw_bps * accuracy_factor / BPS_SCALE^2
        let scale_sq = (BPS_SCALE as u128).pow(2);
        let base_adjusted =
            (base as u128 * rw_bps as u128 * accuracy_factor as u128 / scale_sq) as u64;
        let bonus = reputation.bonus_multiplier_u64();
        base_adjusted * bonus / SCORE_SCALE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_role_percentages_sum_to_ppm_scale() {
        let config = RewardConfig::default();
        let sum: u32 = config.role_percentages_ppm.values().sum();
        assert_eq!(
            sum, PPM_SCALE,
            "default role percentages should sum to {PPM_SCALE}, got {sum}"
        );
    }

    #[test]
    fn rejects_percentages_not_summing_to_one() {
        let mut config = RewardConfig::default();
        let mut pcts = BTreeMap::new();
        pcts.insert(PublishingRole::Analyzer, 7.0);
        pcts.insert(PublishingRole::Collector, 3.0);
        assert!(
            config.set_role_percentages(pcts).is_err(),
            "percentages summing to 10.0 should be rejected"
        );
    }

    #[test]
    fn accepts_percentages_summing_to_one() {
        let mut config = RewardConfig::default();
        let mut pcts = BTreeMap::new();
        pcts.insert(PublishingRole::Analyzer, 0.6);
        pcts.insert(PublishingRole::Collector, 0.4);
        config.set_role_percentages(pcts).expect("should succeed");
        let sum: u32 = config.role_percentages_ppm.values().sum();
        assert_eq!(sum, PPM_SCALE);
    }

    #[test]
    fn rejects_empty_percentages() {
        let mut config = RewardConfig::default();
        let result = config.set_role_percentages(BTreeMap::new());
        assert!(result.is_err(), "empty map should be rejected");
    }

    #[test]
    fn neutral_adjustments_return_base() {
        let config = RewardConfig::default();
        // accuracy=0.5, reputation=0.5 => both factors = 1.0, no region => rw=1.0
        let result = config.apply_adjustments(100, None, 0.5, 0.5);
        assert_eq!(result, 100);
    }

    #[test]
    fn region_boost_doubles_reward() {
        let mut config = RewardConfig::default();
        let mut weights = FxHashMap::default();
        weights.insert("data-desert".into(), 2.0);
        config.set_region_weights(weights);

        let result = config.apply_adjustments(100, Some("data-desert"), 0.5, 0.5);
        assert_eq!(result, 200);
    }

    #[test]
    fn default_region_weight_is_one() {
        let config = RewardConfig::default();
        assert_eq!(config.region_weight_bps("unknown-region"), BPS_SCALE);
    }

    #[test]
    fn default_viral_reward_policies_are_initialized() {
        let config = RewardConfig::default();
        assert_eq!(config.data_desert_policy.min_station_density, 3);
        assert_eq!(config.hyper_local_policy.min_receipt_count, 10);
    }

    #[test]
    fn default_capability_policy_is_disabled() {
        // Issue #735 — capability scoring must ship disabled by default
        // so reward math is unchanged until governance activates it.
        let config = RewardConfig::default();
        assert!(
            config.capability_policy.is_disabled(),
            "default capability policy must be disabled on launch"
        );
        assert_eq!(config.capability_policy.activation_height, u64::MAX);
        assert_eq!(config.capability_policy.max_multiplier_bps, BPS_SCALE);
    }

    #[test]
    fn legacy_reward_config_json_without_capability_policy_deserializes() {
        // Issue #735 — the `#[serde(default)]` attribute on capability_policy
        // must let older on-disk reward configs (captured before the field
        // was added) deserialize cleanly into a disabled default.
        //
        // Emulate "legacy" state by serializing a default config, stripping
        // the `capability_policy` field from the resulting JSON, and
        // confirming round-trip still succeeds.
        let default_config = RewardConfig::default();
        let mut value = serde_json::to_value(&default_config)
            .expect("default config must serialize");
        value
            .as_object_mut()
            .expect("reward config serializes as object")
            .remove("capability_policy");

        let legacy_json = serde_json::to_string(&value).unwrap();
        let config: RewardConfig = serde_json::from_str(&legacy_json)
            .expect("legacy config without capability_policy must deserialize");
        assert!(
            config.capability_policy.is_disabled(),
            "legacy configs must default to disabled capability policy"
        );
    }

    #[test]
    fn reputation_score_u64_bonus_applies_correctly() {
        use wrfcoin_reputation_engine::{ReputationScore, SCORE_SCALE};
        let config = RewardConfig::default();
        // score = 0 → bonus = 1.0x → result = base
        let s0 = ReputationScore(0);
        assert_eq!(config.apply_adjustments_with_score(100, None, 0.5, &s0), 100);
        // score = 100 (max) → bonus = 2.0x → result = 2*base
        let s100 = ReputationScore(100 * SCORE_SCALE);
        assert_eq!(
            config.apply_adjustments_with_score(100, None, 0.5, &s100),
            200
        );
    }

    #[test]
    fn apply_adjustments_deterministic_integer_math() {
        let config = RewardConfig::default();
        // Run the same adjustment 1000 times and verify identical results.
        let expected = config.apply_adjustments(1_000_000, Some("unknown"), 0.8, 0.6);
        for _ in 0..1_000 {
            assert_eq!(
                config.apply_adjustments(1_000_000, Some("unknown"), 0.8, 0.6),
                expected
            );
        }
    }

    #[test]
    fn ppm_setter_rejects_wrong_sum() {
        let mut config = RewardConfig::default();
        let mut pcts = BTreeMap::new();
        pcts.insert(PublishingRole::Analyzer, 500_000);
        pcts.insert(PublishingRole::Collector, 400_000);
        // Sum = 900_000, not 1_000_000
        assert!(config.set_role_percentages_ppm(pcts).is_err());
    }

    #[test]
    fn ppm_setter_accepts_correct_sum() {
        let mut config = RewardConfig::default();
        let mut pcts = BTreeMap::new();
        pcts.insert(PublishingRole::Analyzer, 600_000);
        pcts.insert(PublishingRole::Collector, 400_000);
        config.set_role_percentages_ppm(pcts).expect("should succeed");
        assert_eq!(config.role_percentages_ppm.values().sum::<u32>(), PPM_SCALE);
    }
}
