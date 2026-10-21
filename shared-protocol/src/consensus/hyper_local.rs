use crate::consensus::proof_of_publishing::MAX_COMBINED_MULTIPLIER_BPS;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use wrfcoin_geo_grid::GeoCell;

/// Resolution in kilometres matching the desert density grid (≈ 2° at the equator).
const HYPER_LOCAL_RESOLUTION_KM: u8 = 222;

/// Policy governing hyper-local accuracy bonuses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperLocalPolicy {
    /// Minimum number of verified receipts needed to qualify.
    pub min_receipt_count: u32,
    /// Accuracy score threshold (0-10000 bps) to earn the bonus.
    pub accuracy_threshold_bps: u32,
    /// Bonus multiplier in basis points (added to 10000 base).
    pub accuracy_bonus_bps: u32,
    /// How many recent receipts to consider (sliding window).
    pub window_size: u32,
    /// Block height activation gate.
    pub activation_height: u64,
}

impl Default for HyperLocalPolicy {
    fn default() -> Self {
        Self {
            min_receipt_count: 10,
            accuracy_threshold_bps: 8500,
            accuracy_bonus_bps: 2000,
            window_size: 50,
            activation_height: 0,
        }
    }
}

/// Per-oracle accuracy summary for a geographic cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleLocalAccuracy {
    pub oracle_id: String,
    pub cell: GeoCell,
    /// Rolling window of accuracy scores from forecast verification receipts (0-10000 bps).
    pub recent_scores: VecDeque<u32>,
    pub receipt_count: u32,
}

impl Default for OracleLocalAccuracy {
    fn default() -> Self {
        Self {
            oracle_id: String::new(),
            // GeoCell has no Default impl (resolution_km=0 is invalid), so we use the
            // same 222 km resolution as the desert grid.
            cell: GeoCell { lat_idx: 0, lon_idx: 0, resolution_km: HYPER_LOCAL_RESOLUTION_KM },
            recent_scores: VecDeque::new(),
            receipt_count: 0,
        }
    }
}

impl OracleLocalAccuracy {
    pub fn push_score(&mut self, score_bps: u32, window: u32) {
        self.receipt_count = self.receipt_count.saturating_add(1);
        self.recent_scores.push_back(score_bps.min(10_000));

        let max_len = if window == 0 { 50 } else { window as usize };
        while self.recent_scores.len() > max_len {
            let _ = self.recent_scores.pop_front();
        }
    }

    /// Mean accuracy over the window, in basis points.
    pub fn mean_accuracy_bps(&self) -> u32 {
        if self.recent_scores.is_empty() {
            return 0;
        }

        let sum: u64 = self.recent_scores.iter().map(|v| *v as u64).sum();
        (sum / self.recent_scores.len() as u64) as u32
    }
}

/// Returns the hyper-local multiplier in basis points (10000 = no bonus).
pub fn hyper_local_multiplier(
    accuracy: &OracleLocalAccuracy,
    policy: &HyperLocalPolicy,
    current_height: u64,
) -> u32 {
    if current_height < policy.activation_height {
        return 10_000;
    }

    if accuracy.receipt_count < policy.min_receipt_count {
        return 10_000;
    }

    if accuracy.mean_accuracy_bps() < policy.accuracy_threshold_bps {
        return 10_000;
    }

    10_000u32.saturating_add(policy.accuracy_bonus_bps)
}

/// Compose reward multipliers safely using u128 intermediates.
/// The combined factor (desert × hyper-local / 100) is clamped to
/// `MAX_COMBINED_MULTIPLIER_BPS` so the net multiplier cannot exceed 2.0x.
pub fn compose_reward_with_multipliers(
    base_reward: u128,
    desert_mult_bps: u32,
    hyper_local_mult_bps: u32,
) -> u128 {
    let combined_bps = (desert_mult_bps as u128)
        .saturating_mul(hyper_local_mult_bps as u128)
        .saturating_div(100)
        .min(MAX_COMBINED_MULTIPLIER_BPS);

    base_reward
        .saturating_mul(combined_bps)
        .saturating_div(10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accuracy_with_scores(scores: &[u32]) -> OracleLocalAccuracy {
        let mut accuracy = OracleLocalAccuracy {
            oracle_id: "oracle-1".to_string(),
            cell: GeoCell { lat_idx: 10, lon_idx: -20, resolution_km: HYPER_LOCAL_RESOLUTION_KM },
            ..OracleLocalAccuracy::default()
        };

        for score in scores {
            accuracy.push_score(*score, 50);
        }

        accuracy
    }

    #[test]
    fn oracle_with_ten_high_scores_gets_bonus() {
        let policy = HyperLocalPolicy::default();
        let accuracy = accuracy_with_scores(&[9000; 10]);
        assert_eq!(hyper_local_multiplier(&accuracy, &policy, 100), 12_000);
    }

    #[test]
    fn oracle_with_ten_scores_below_threshold_gets_no_bonus() {
        let policy = HyperLocalPolicy::default();
        let accuracy = accuracy_with_scores(&[8000; 10]);
        assert_eq!(hyper_local_multiplier(&accuracy, &policy, 100), 10_000);
    }

    #[test]
    fn oracle_with_insufficient_receipts_gets_no_bonus() {
        let policy = HyperLocalPolicy::default();
        let accuracy = accuracy_with_scores(&[9000; 5]);
        assert_eq!(hyper_local_multiplier(&accuracy, &policy, 100), 10_000);
    }

    #[test]
    fn before_activation_returns_baseline() {
        let policy = HyperLocalPolicy {
            activation_height: 1_000,
            ..HyperLocalPolicy::default()
        };
        let accuracy = accuracy_with_scores(&[9000; 10]);
        assert_eq!(hyper_local_multiplier(&accuracy, &policy, 999), 10_000);
    }

    #[test]
    fn sliding_window_evicts_oldest_scores() {
        let mut accuracy = OracleLocalAccuracy {
            oracle_id: "oracle-1".to_string(),
            cell: GeoCell { lat_idx: 0, lon_idx: 0, resolution_km: HYPER_LOCAL_RESOLUTION_KM },
            ..OracleLocalAccuracy::default()
        };

        accuracy.push_score(7000, 3);
        accuracy.push_score(8000, 3);
        accuracy.push_score(9000, 3);
        accuracy.push_score(10_000, 3);

        assert_eq!(accuracy.recent_scores.len(), 3);
        assert_eq!(accuracy.recent_scores[0], 8000);
        assert_eq!(accuracy.recent_scores[2], 10_000);
    }

    #[test]
    fn multiplier_composition_uses_u128_intermediates() {
        let base = u128::MAX / 2;
        let reward = compose_reward_with_multipliers(base, u32::MAX, u32::MAX);
        assert!(reward > 0);

        let small = compose_reward_with_multipliers(1_000, 150, 12_000);
        assert_eq!(small, 1_800);
    }

    #[test]
    fn push_score_zero_window_uses_default_cap() {
        let mut accuracy = OracleLocalAccuracy {
            oracle_id: "test".into(),
            cell: GeoCell { lat_idx: 0, lon_idx: 0, resolution_km: 2 },
            recent_scores: VecDeque::new(),
            receipt_count: 0,
        };
        for i in 0..100 {
            accuracy.push_score(i * 100, 0);
        }
        assert_eq!(accuracy.recent_scores.len(), 50);
        assert_eq!(accuracy.receipt_count, 100);
    }

    #[test]
    fn combined_multiplier_capped_at_2x() {
        // desert=200 (2.0x), hyper_local=12_000 (1.2x) → raw combined = 200*12000/100 = 24_000
        // Clamped to MAX_COMBINED_MULTIPLIER_BPS = 20_000
        let reward = compose_reward_with_multipliers(1_000_000, 200, 12_000);
        // 1_000_000 * 20_000 / 10_000 = 2_000_000 (2.0x cap)
        assert_eq!(reward, 2_000_000);
    }

    #[test]
    fn combined_multiplier_below_cap_unchanged() {
        // desert=150 (1.5x), hyper_local=10_000 (1.0x) → combined = 150*10_000/100 = 15_000
        let reward = compose_reward_with_multipliers(1_000_000, 150, 10_000);
        // 1_000_000 * 15_000 / 10_000 = 1_500_000 (1.5x, within cap)
        assert_eq!(reward, 1_500_000);
    }
}
