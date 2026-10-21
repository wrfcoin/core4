use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::consensus::proof_of_publishing::PublishingRole;
use crate::rewards::config::BPS_SCALE;

/// Itemised breakdown of how a final reward was computed, including per-role
/// distribution and the individual adjustment factors applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardBreakdown {
    /// The base emission amount before adjustments.
    pub base_emission: u64,

    /// Reward allocated to each publishing role (sums to `final_reward`).
    /// Uses BTreeMap for deterministic iteration order in consensus-critical paths.
    pub role_distribution: BTreeMap<PublishingRole, u64>,

    /// Region weight factor that was applied (BPS, 10_000 = 1.0x).
    pub region_factor_bps: u32,

    /// Accuracy adjustment factor that was applied (BPS, 10_000 = 1.0x).
    pub accuracy_factor_bps: u32,

    /// Reputation adjustment factor that was applied (BPS, 10_000 = 1.0x).
    pub reputation_factor_bps: u32,

    /// The final reward after all adjustments.
    pub final_reward: u64,
}

impl RewardBreakdown {
    /// Create a simple breakdown with all factors set to 10_000 BPS (1.0x neutral).
    pub fn simple(base_emission: u64, final_reward: u64) -> Self {
        Self {
            base_emission,
            role_distribution: BTreeMap::new(),
            region_factor_bps: BPS_SCALE,
            accuracy_factor_bps: BPS_SCALE,
            reputation_factor_bps: BPS_SCALE,
            final_reward,
        }
    }

    /// Distribute `final_reward` across publishing roles according to the
    /// given PPM (parts-per-million) map. Uses deterministic BTreeMap iteration
    /// order and pure integer arithmetic. The last role (by BTreeMap sort order)
    /// absorbs any rounding remainder so the distribution sums exactly to
    /// `final_reward`.
    pub fn distribute_roles(
        &mut self,
        role_percentages_ppm: &BTreeMap<PublishingRole, u32>,
    ) {
        self.role_distribution.clear();
        if role_percentages_ppm.is_empty() {
            return;
        }

        let ppm_total: u64 = role_percentages_ppm.values().map(|&p| p as u64).sum();

        let mut distributed: u64 = 0;
        let last_idx = role_percentages_ppm.len() - 1;

        for (i, (&role, &ppm)) in role_percentages_ppm.iter().enumerate() {
            if i == last_idx {
                // Last role gets the remainder to avoid rounding drift.
                self.role_distribution
                    .insert(role, self.final_reward.saturating_sub(distributed));
            } else {
                let share = self
                    .final_reward
                    .checked_mul(ppm as u64)
                    .map(|v| v / ppm_total)
                    .unwrap_or(0);
                self.role_distribution.insert(role, share);
                distributed = distributed.saturating_add(share);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_breakdown_values() {
        let bd = RewardBreakdown::simple(500, 1000);
        assert_eq!(bd.base_emission, 500);
        assert_eq!(bd.final_reward, 1000);
        assert_eq!(bd.region_factor_bps, BPS_SCALE);
        assert_eq!(bd.accuracy_factor_bps, BPS_SCALE);
        assert_eq!(bd.reputation_factor_bps, BPS_SCALE);
        assert!(bd.role_distribution.is_empty());
    }

    #[test]
    fn distribute_roles_sums_correctly() {
        let mut bd = RewardBreakdown::simple(1000, 1000);
        let mut pcts = BTreeMap::new();
        pcts.insert(PublishingRole::Analyzer, 350_000u32);
        pcts.insert(PublishingRole::Collector, 300_000);
        pcts.insert(PublishingRole::Keeper, 250_000);
        pcts.insert(PublishingRole::Offsetter, 100_000);

        bd.distribute_roles(&pcts);

        let sum: u64 = bd.role_distribution.values().sum();
        assert_eq!(
            sum, 1000,
            "role distribution must sum to final_reward, got {sum}"
        );
        assert_eq!(bd.role_distribution.len(), 4);
    }
}
