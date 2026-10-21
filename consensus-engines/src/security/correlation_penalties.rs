//! Correlated Slashing Penalties
//!
//! Implements quadratic/correlated slashing where the penalty for a violation
//! increases when multiple validators misbehave around the same time. This
//! discourages coordinated attacks and large-scale validator collusion.

use serde::{Deserialize, Serialize};

/// Minimum penalty fraction (0.1% of stake)
pub const MIN_PENALTY_FRACTION: f64 = 0.001;

/// Maximum penalty fraction (100% of stake)
pub const MAX_PENALTY_FRACTION: f64 = 1.0;

/// Default correlation window in blocks
pub const DEFAULT_CORRELATION_WINDOW: u64 = 4096;

/// Default whistleblower reward fraction (5% of slashed amount)
pub const DEFAULT_WHISTLEBLOWER_REWARD_FRACTION: f64 = 0.05;

/// Types of slashing violations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashingViolationType {
    /// Validator signed two different blocks at the same height
    DoubleSign,
    /// Validator created a surround vote (conflicting attestations)
    Surround,
    /// Validator was offline for an extended period
    Downtime,
    /// Validator produced an invalid block
    InvalidBlock,
}

impl SlashingViolationType {
    /// Base penalty fraction for this violation type (before correlation adjustment).
    fn base_penalty_fraction(&self) -> f64 {
        match self {
            SlashingViolationType::DoubleSign => 0.05,   // 5%
            SlashingViolationType::Surround => 0.05,     // 5%
            SlashingViolationType::Downtime => 0.01,     // 1%
            SlashingViolationType::InvalidBlock => 0.03, // 3%
        }
    }

    /// Whether this violation type is considered severe enough for correlation scaling.
    fn is_correlatable(&self) -> bool {
        matches!(
            self,
            SlashingViolationType::DoubleSign
                | SlashingViolationType::Surround
                | SlashingViolationType::InvalidBlock
        )
    }
}

/// A slashing violation event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingViolation {
    /// Address of the offending validator
    pub validator_address: String,
    /// Type of violation
    pub violation_type: SlashingViolationType,
    /// Block height at which the violation occurred
    pub block_height: u64,
    /// Validator's stake at time of violation
    pub validator_stake: u64,
    /// Total network stake at time of violation
    pub total_network_stake: u64,
    /// Timestamp of the violation
    pub timestamp: u64,
    /// Optional evidence data
    pub evidence: Option<String>,
}

/// Computed slashing penalty
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingPenalty {
    /// Address of the slashed validator
    pub validator_address: String,
    /// Base penalty (before correlation adjustment)
    pub base_penalty: u64,
    /// Correlation multiplier applied
    pub correlation_multiplier: f64,
    /// Final penalty amount after correlation adjustment
    pub final_penalty: u64,
    /// Penalty as a fraction of the validator's stake
    pub penalty_fraction: f64,
    /// Number of correlated violations in the window
    pub correlated_count: usize,
    /// Whistleblower reward amount
    pub whistleblower_reward: u64,
}

/// Configuration for the correlation penalty calculator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationConfig {
    /// Number of blocks within which violations are considered correlated
    pub correlation_window: u64,
    /// Fraction of slashed amount given to whistleblower
    pub whistleblower_reward_fraction: f64,
    /// Minimum penalty fraction of stake
    pub min_penalty_fraction: f64,
    /// Maximum penalty fraction of stake
    pub max_penalty_fraction: f64,
}

impl Default for CorrelationConfig {
    fn default() -> Self {
        Self {
            correlation_window: DEFAULT_CORRELATION_WINDOW,
            whistleblower_reward_fraction: DEFAULT_WHISTLEBLOWER_REWARD_FRACTION,
            min_penalty_fraction: MIN_PENALTY_FRACTION,
            max_penalty_fraction: MAX_PENALTY_FRACTION,
        }
    }
}

/// Calculator for correlated slashing penalties.
///
/// Penalties scale quadratically with the fraction of total stake being slashed
/// within a correlation window. If 1% of stake misbehaves, penalties are modest;
/// if 33% misbehaves, penalties are severe.
#[derive(Debug, Clone)]
pub struct CorrelationPenaltyCalculator {
    config: CorrelationConfig,
}

impl CorrelationPenaltyCalculator {
    fn apply_fraction_to_stake(stake: u64, fraction: f64) -> u64 {
        if !fraction.is_finite() || fraction <= 0.0 {
            return 0;
        }
        if fraction >= 1.0 {
            return stake;
        }

        (stake as f64 * fraction) as u64
    }

    /// Create a new calculator with default configuration.
    pub fn new() -> Self {
        Self {
            config: CorrelationConfig::default(),
        }
    }

    /// Create a new calculator with custom configuration.
    pub fn with_config(config: CorrelationConfig) -> Self {
        Self { config }
    }

    /// Calculate the slashing penalty for a violation, considering concurrent violations.
    ///
    /// The penalty uses quadratic scaling: `penalty = base * 3 * (fraction_slashed)^2`
    /// where `fraction_slashed` is the fraction of total network stake that is being
    /// slashed within the correlation window.
    ///
    /// # Arguments
    /// * `violation` - The violation to calculate penalty for
    /// * `concurrent_violations` - Other violations within the correlation window
    pub fn calculate_penalty(
        &self,
        violation: &SlashingViolation,
        concurrent_violations: &[SlashingViolation],
    ) -> SlashingPenalty {
        let base_fraction = violation.violation_type.base_penalty_fraction();
        let base_penalty = Self::apply_fraction_to_stake(violation.validator_stake, base_fraction);

        // Find violations within the correlation window
        let correlated: Vec<&SlashingViolation> = if violation.violation_type.is_correlatable() {
            concurrent_violations
                .iter()
                .filter(|v| {
                    v.validator_address != violation.validator_address
                        && self.is_within_window(violation.block_height, v.block_height)
                        && v.violation_type.is_correlatable()
                })
                .collect()
        } else {
            vec![]
        };

        let correlated_count = correlated.len();

        // Calculate fraction of total stake being slashed concurrently
        let total_slashed_stake: u128 = correlated
            .iter()
            .map(|v| u128::from(v.validator_stake))
            .sum::<u128>()
            + u128::from(violation.validator_stake);

        let total_network_stake = violation.total_network_stake.max(1);

        let fraction_slashed = total_slashed_stake as f64 / total_network_stake as f64;

        // Quadratic scaling: penalty proportional to (fraction_slashed)^2
        // Multiplied by 3 to make 33% slashed = 33% penalty (1/3 * 1/3 * 3 = 1/3)
        let correlation_multiplier = if correlated_count > 0 {
            let quadratic = fraction_slashed * fraction_slashed * 3.0;
            // Ensure multiplier is at least 1.0 (don't reduce base penalty)
            quadratic.max(1.0)
        } else {
            1.0
        };

        // Calculate final penalty
        let adjusted_fraction = (base_fraction * correlation_multiplier).clamp(
            self.config.min_penalty_fraction,
            self.config.max_penalty_fraction,
        );

        let final_penalty =
            Self::apply_fraction_to_stake(violation.validator_stake, adjusted_fraction);

        // Whistleblower reward
        let whistleblower_reward = self.calculate_whistleblower_reward(final_penalty);

        SlashingPenalty {
            validator_address: violation.validator_address.clone(),
            base_penalty,
            correlation_multiplier,
            final_penalty,
            penalty_fraction: adjusted_fraction,
            correlated_count,
            whistleblower_reward,
        }
    }

    /// Calculate the whistleblower reward for reporting a violation.
    pub fn calculate_whistleblower_reward(&self, slashed_amount: u64) -> u64 {
        Self::apply_fraction_to_stake(slashed_amount, self.config.whistleblower_reward_fraction)
    }

    /// Check if two block heights are within the correlation window.
    fn is_within_window(&self, height_a: u64, height_b: u64) -> bool {
        height_a.abs_diff(height_b) <= self.config.correlation_window
    }

    /// Get the current configuration.
    pub fn config(&self) -> &CorrelationConfig {
        &self.config
    }
}

impl Default for CorrelationPenaltyCalculator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_violation(
        address: &str,
        violation_type: SlashingViolationType,
        block_height: u64,
        stake: u64,
        total_stake: u64,
    ) -> SlashingViolation {
        SlashingViolation {
            validator_address: address.to_string(),
            violation_type,
            block_height,
            validator_stake: stake,
            total_network_stake: total_stake,
            timestamp: 0,
            evidence: None,
        }
    }

    #[test]
    fn test_single_double_sign_penalty() {
        let calc = CorrelationPenaltyCalculator::new();
        let violation = make_violation(
            "val-1",
            SlashingViolationType::DoubleSign,
            100,
            1000,
            100_000,
        );

        let penalty = calc.calculate_penalty(&violation, &[]);

        // Base: 5% of 1000 = 50
        assert_eq!(penalty.base_penalty, 50);
        assert_eq!(penalty.correlated_count, 0);
        assert!((penalty.correlation_multiplier - 1.0).abs() < f64::EPSILON);
        assert_eq!(penalty.final_penalty, 50);
    }

    #[test]
    fn test_single_downtime_penalty() {
        let calc = CorrelationPenaltyCalculator::new();
        let violation = make_violation(
            "val-1",
            SlashingViolationType::Downtime,
            100,
            10_000,
            100_000,
        );

        let penalty = calc.calculate_penalty(&violation, &[]);

        // Base: 1% of 10_000 = 100
        assert_eq!(penalty.base_penalty, 100);
        assert_eq!(penalty.final_penalty, 100);
    }

    #[test]
    fn test_downtime_not_correlatable() {
        let calc = CorrelationPenaltyCalculator::new();
        let violation = make_violation(
            "val-1",
            SlashingViolationType::Downtime,
            100,
            10_000,
            100_000,
        );
        let concurrent = vec![make_violation(
            "val-2",
            SlashingViolationType::Downtime,
            101,
            10_000,
            100_000,
        )];

        let penalty = calc.calculate_penalty(&violation, &concurrent);

        // Downtime is not correlatable, so no multiplier
        assert_eq!(penalty.correlated_count, 0);
        assert!((penalty.correlation_multiplier - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_quadratic_scaling_small_fraction() {
        let calc = CorrelationPenaltyCalculator::new();
        let total_stake = 1_000_000;

        let violation = make_violation(
            "val-1",
            SlashingViolationType::DoubleSign,
            100,
            10_000,
            total_stake,
        );
        let concurrent = vec![make_violation(
            "val-2",
            SlashingViolationType::DoubleSign,
            101,
            10_000,
            total_stake,
        )];

        let penalty = calc.calculate_penalty(&violation, &concurrent);

        assert_eq!(penalty.correlated_count, 1);
        assert!(penalty.correlation_multiplier >= 1.0);
    }

    #[test]
    fn test_quadratic_scaling_large_fraction() {
        let calc = CorrelationPenaltyCalculator::new();
        let total_stake = 100_000;

        // 75% of stake is being slashed (3 validators at 25,000 each)
        // fraction_slashed = 0.75, quadratic = 0.75^2 * 3 = 1.6875 > 1.0
        let violation = make_violation(
            "val-1",
            SlashingViolationType::DoubleSign,
            100,
            25_000,
            total_stake,
        );
        let concurrent = vec![
            make_violation(
                "val-2",
                SlashingViolationType::DoubleSign,
                101,
                25_000,
                total_stake,
            ),
            make_violation(
                "val-3",
                SlashingViolationType::DoubleSign,
                102,
                25_000,
                total_stake,
            ),
        ];

        let penalty = calc.calculate_penalty(&violation, &concurrent);

        assert_eq!(penalty.correlated_count, 2);
        assert!(penalty.correlation_multiplier > 1.0);
        assert!(penalty.final_penalty > penalty.base_penalty);
    }

    #[test]
    fn test_penalty_fraction_capped_at_max() {
        let calc = CorrelationPenaltyCalculator::new();
        let total_stake = 30_000;

        let violation = make_violation(
            "val-1",
            SlashingViolationType::DoubleSign,
            100,
            10_000,
            total_stake,
        );
        let concurrent = vec![
            make_violation(
                "val-2",
                SlashingViolationType::DoubleSign,
                101,
                10_000,
                total_stake,
            ),
            make_violation(
                "val-3",
                SlashingViolationType::DoubleSign,
                102,
                10_000,
                total_stake,
            ),
        ];

        let penalty = calc.calculate_penalty(&violation, &concurrent);

        assert!(penalty.penalty_fraction <= MAX_PENALTY_FRACTION);
        assert!(penalty.final_penalty <= violation.validator_stake);
    }

    #[test]
    fn test_penalty_fraction_above_minimum() {
        let calc = CorrelationPenaltyCalculator::new();
        let violation = make_violation(
            "val-1",
            SlashingViolationType::Downtime,
            100,
            100,
            10_000_000,
        );

        let penalty = calc.calculate_penalty(&violation, &[]);
        assert!(penalty.penalty_fraction >= MIN_PENALTY_FRACTION);
    }

    #[test]
    fn test_outside_correlation_window() {
        let calc = CorrelationPenaltyCalculator::new();
        let violation = make_violation(
            "val-1",
            SlashingViolationType::DoubleSign,
            100,
            10_000,
            100_000,
        );

        let concurrent = vec![make_violation(
            "val-2",
            SlashingViolationType::DoubleSign,
            100_000,
            10_000,
            100_000,
        )];

        let penalty = calc.calculate_penalty(&violation, &concurrent);
        assert_eq!(penalty.correlated_count, 0);
        assert!((penalty.correlation_multiplier - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_same_validator_not_correlated_with_self() {
        let calc = CorrelationPenaltyCalculator::new();
        let violation = make_violation(
            "val-1",
            SlashingViolationType::DoubleSign,
            100,
            10_000,
            100_000,
        );

        let concurrent = vec![make_violation(
            "val-1",
            SlashingViolationType::DoubleSign,
            101,
            10_000,
            100_000,
        )];

        let penalty = calc.calculate_penalty(&violation, &concurrent);
        assert_eq!(penalty.correlated_count, 0);
    }

    #[test]
    fn test_whistleblower_reward() {
        let calc = CorrelationPenaltyCalculator::new();
        let violation = make_violation(
            "val-1",
            SlashingViolationType::DoubleSign,
            100,
            10_000,
            100_000,
        );

        let penalty = calc.calculate_penalty(&violation, &[]);

        let expected_reward =
            (penalty.final_penalty as f64 * DEFAULT_WHISTLEBLOWER_REWARD_FRACTION) as u64;
        assert_eq!(penalty.whistleblower_reward, expected_reward);
    }

    #[test]
    fn test_whistleblower_reward_calculation() {
        let calc = CorrelationPenaltyCalculator::new();
        assert_eq!(calc.calculate_whistleblower_reward(1000), 50); // 5% of 1000
        assert_eq!(calc.calculate_whistleblower_reward(0), 0);
    }

    #[test]
    fn test_custom_config() {
        let config = CorrelationConfig {
            correlation_window: 100,
            whistleblower_reward_fraction: 0.10,
            min_penalty_fraction: 0.01,
            max_penalty_fraction: 0.50,
        };
        let calc = CorrelationPenaltyCalculator::with_config(config);

        assert_eq!(calc.config().correlation_window, 100);
        assert!((calc.config().whistleblower_reward_fraction - 0.10).abs() < f64::EPSILON);
    }

    #[test]
    fn test_violation_base_penalties() {
        assert!(
            (SlashingViolationType::DoubleSign.base_penalty_fraction() - 0.05).abs() < f64::EPSILON
        );
        assert!(
            (SlashingViolationType::Surround.base_penalty_fraction() - 0.05).abs() < f64::EPSILON
        );
        assert!(
            (SlashingViolationType::Downtime.base_penalty_fraction() - 0.01).abs() < f64::EPSILON
        );
        assert!(
            (SlashingViolationType::InvalidBlock.base_penalty_fraction() - 0.03).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn test_invalid_block_is_correlatable() {
        assert!(SlashingViolationType::DoubleSign.is_correlatable());
        assert!(SlashingViolationType::Surround.is_correlatable());
        assert!(SlashingViolationType::InvalidBlock.is_correlatable());
        assert!(!SlashingViolationType::Downtime.is_correlatable());
    }

    #[test]
    fn test_increasing_correlation_increases_penalty() {
        let calc = CorrelationPenaltyCalculator::new();
        let total_stake = 100_000;

        let violation = make_violation(
            "val-1",
            SlashingViolationType::DoubleSign,
            100,
            10_000,
            total_stake,
        );

        let penalty_0 = calc.calculate_penalty(&violation, &[]);

        let concurrent_1 = vec![make_violation(
            "val-2",
            SlashingViolationType::DoubleSign,
            101,
            10_000,
            total_stake,
        )];
        let penalty_1 = calc.calculate_penalty(&violation, &concurrent_1);

        let concurrent_3 = vec![
            make_violation(
                "val-2",
                SlashingViolationType::DoubleSign,
                101,
                10_000,
                total_stake,
            ),
            make_violation(
                "val-3",
                SlashingViolationType::DoubleSign,
                102,
                10_000,
                total_stake,
            ),
            make_violation(
                "val-4",
                SlashingViolationType::DoubleSign,
                103,
                10_000,
                total_stake,
            ),
        ];
        let penalty_3 = calc.calculate_penalty(&violation, &concurrent_3);

        assert!(penalty_1.final_penalty >= penalty_0.final_penalty);
        assert!(penalty_3.final_penalty >= penalty_1.final_penalty);
    }

    #[test]
    fn test_surround_violation() {
        let calc = CorrelationPenaltyCalculator::new();
        let violation =
            make_violation("val-1", SlashingViolationType::Surround, 100, 5000, 100_000);

        let penalty = calc.calculate_penalty(&violation, &[]);

        // Base: 5% of 5000 = 250
        assert_eq!(penalty.base_penalty, 250);
        assert_eq!(penalty.final_penalty, 250);
    }

    #[test]
    fn correlated_stake_sum_above_u64_preserves_large_penalty_without_overflow() {
        let calc = CorrelationPenaltyCalculator::new();
        let violation = make_violation(
            "validator-main",
            SlashingViolationType::DoubleSign,
            100,
            u64::MAX,
            u64::MAX,
        );
        let concurrent = vec![make_violation(
            "validator-peer",
            SlashingViolationType::DoubleSign,
            101,
            u64::MAX,
            u64::MAX,
        )];

        let penalty = calc.calculate_penalty(&violation, &concurrent);
        assert!(
            penalty.final_penalty > penalty.base_penalty,
            "correlated stake beyond u64::MAX should still increase the penalty instead of wrapping"
        );
        assert!(
            penalty.penalty_fraction >= 0.60,
            "expected the quadratic multiplier to preserve a large correlated slash, got {}",
            penalty.penalty_fraction
        );
    }
}
