//! EIP-1559 Dynamic Fee Market
//!
//! Implements the EIP-1559 fee mechanism with base fee adjustment, priority fees,
//! and fee burning. The base fee adjusts automatically based on block gas usage
//! relative to a target, providing more predictable transaction fees.
//!
//! Extracted from `blockchain-core/src/consensus/eip1559_fee_market.rs`.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Transaction types supported by the fee market
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionType {
    /// Legacy pre-EIP-1559 transaction with a single gas price
    Legacy,
    /// EIP-2930 transaction with access list
    Eip2930,
    /// EIP-1559 transaction with max fee and priority fee
    Eip1559,
}

/// Block-level gas statistics for fee estimation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockStats {
    /// Gas used in this block
    pub gas_used: u64,
    /// Gas target for this block
    pub gas_target: u64,
    /// Base fee of this block
    pub base_fee: u64,
    /// Block number
    pub block_number: u64,
}

/// Result of effective fee calculation for a transaction
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveFee {
    /// The effective gas price paid by the transaction
    pub effective_gas_price: u64,
    /// The priority fee (tip) going to the validator
    pub priority_fee: u64,
    /// The base fee portion that gets burned
    pub base_fee: u64,
}

/// Fee estimate at different urgency levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeEstimate {
    /// Low urgency: may take several blocks to confirm
    pub low: FeeLevel,
    /// Medium urgency: likely to confirm within a few blocks
    pub medium: FeeLevel,
    /// High urgency: likely to confirm in the next block
    pub high: FeeLevel,
    /// Current base fee
    pub current_base_fee: u64,
}

/// Fee parameters for a given urgency level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeLevel {
    /// Suggested max fee per gas
    pub max_fee_per_gas: u64,
    /// Suggested max priority fee per gas
    pub max_priority_fee_per_gas: u64,
}

/// EIP-1559 fee market implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeMarket {
    /// Current base fee per gas
    pub base_fee: u64,
    /// Minimum allowed base fee
    pub min_base_fee: u64,
    /// Maximum allowed base fee
    pub max_base_fee: u64,
    /// Elasticity multiplier: max gas = target * elasticity
    pub elasticity_multiplier: u64,
    /// Base fee change denominator (dampening factor)
    pub base_fee_change_denominator: u64,
}

impl Default for FeeMarket {
    fn default() -> Self {
        Self {
            base_fee: 1_000_000_000,       // 1 Gwei
            min_base_fee: 1_000_000,       // 0.001 Gwei
            max_base_fee: 500_000_000_000, // 500 Gwei
            elasticity_multiplier: 2,
            base_fee_change_denominator: 8,
        }
    }
}

impl FeeMarket {
    /// Create a new fee market with custom parameters.
    pub fn new(initial_base_fee: u64, min_base_fee: u64, max_base_fee: u64) -> Self {
        Self {
            base_fee: initial_base_fee,
            min_base_fee,
            max_base_fee,
            elasticity_multiplier: 2,
            base_fee_change_denominator: 8,
        }
    }

    /// Update the base fee based on parent block gas usage.
    ///
    /// The base fee increases when blocks are fuller than target and decreases
    /// when they are emptier. The maximum change per block is 1/8 (12.5%).
    pub fn update_base_fee(
        &mut self,
        parent_gas_used: u64,
        parent_gas_target: u64,
        parent_base_fee: u64,
    ) -> u64 {
        if parent_gas_target == 0 {
            return parent_base_fee;
        }

        let new_base_fee = if parent_gas_used == parent_gas_target {
            // Exactly at target: no change
            parent_base_fee
        } else if parent_gas_used > parent_gas_target {
            // Above target: increase base fee
            let gas_used_delta = parent_gas_used - parent_gas_target;
            let fee_delta = parent_base_fee.saturating_mul(gas_used_delta)
                / parent_gas_target
                / self.base_fee_change_denominator;
            // Ensure at least a 1-unit increase when above target
            parent_base_fee.saturating_add(fee_delta.max(1))
        } else {
            // Below target: decrease base fee
            let gas_used_delta = parent_gas_target - parent_gas_used;
            let fee_delta = parent_base_fee.saturating_mul(gas_used_delta)
                / parent_gas_target
                / self.base_fee_change_denominator;
            parent_base_fee.saturating_sub(fee_delta)
        };

        // Clamp to configured bounds
        let clamped = new_base_fee.clamp(self.min_base_fee, self.max_base_fee);
        self.base_fee = clamped;
        clamped
    }

    /// Calculate the effective fee components for a transaction.
    pub fn calculate_effective_fee(
        &self,
        max_fee_per_gas: u64,
        max_priority_fee: u64,
        base_fee: u64,
    ) -> Option<EffectiveFee> {
        if max_fee_per_gas < base_fee {
            return None;
        }

        // Priority fee is the lesser of max_priority_fee and (max_fee - base_fee)
        let effective_priority_fee = max_priority_fee.min(max_fee_per_gas - base_fee);

        let effective_gas_price = base_fee + effective_priority_fee;

        Some(EffectiveFee {
            effective_gas_price,
            priority_fee: effective_priority_fee,
            base_fee,
        })
    }

    /// Calculate how much fee is burned for a block.
    ///
    /// The burned amount is `base_fee * gas_used`. This is removed from
    /// circulation, creating deflationary pressure.
    pub fn calculate_fee_burn(&self, base_fee: u64, gas_used: u64) -> u64 {
        base_fee.saturating_mul(gas_used)
    }

    /// Estimate fees for different urgency levels based on recent block history.
    pub fn estimate_fee(&self, pending_blocks: &[BlockStats]) -> FeeEstimate {
        if pending_blocks.is_empty() {
            return FeeEstimate {
                low: FeeLevel {
                    max_fee_per_gas: self.base_fee,
                    max_priority_fee_per_gas: 1_000_000_000, // 1 Gwei default tip
                },
                medium: FeeLevel {
                    max_fee_per_gas: self.base_fee.saturating_mul(2),
                    max_priority_fee_per_gas: 2_000_000_000,
                },
                high: FeeLevel {
                    max_fee_per_gas: self.base_fee.saturating_mul(3),
                    max_priority_fee_per_gas: 5_000_000_000,
                },
                current_base_fee: self.base_fee,
            };
        }

        // Analyze recent congestion
        let avg_utilization = self.calculate_avg_utilization(pending_blocks);
        let latest_base_fee = pending_blocks
            .last()
            .map(|b| b.base_fee)
            .unwrap_or(self.base_fee);

        // Project future base fee based on utilization trend
        let projected_increase = if avg_utilization > 100 {
            let excess = avg_utilization - 100;
            latest_base_fee.saturating_mul(excess) / 100 / self.base_fee_change_denominator
        } else {
            0
        };

        let projected_base_fee = latest_base_fee.saturating_add(projected_increase);

        FeeEstimate {
            low: FeeLevel {
                max_fee_per_gas: projected_base_fee.saturating_add(
                    projected_base_fee / 10, // 10% buffer
                ),
                max_priority_fee_per_gas: 1_000_000_000,
            },
            medium: FeeLevel {
                max_fee_per_gas: projected_base_fee.saturating_add(
                    projected_base_fee / 4, // 25% buffer
                ),
                max_priority_fee_per_gas: 2_000_000_000,
            },
            high: FeeLevel {
                max_fee_per_gas: projected_base_fee.saturating_add(
                    projected_base_fee / 2, // 50% buffer
                ),
                max_priority_fee_per_gas: 5_000_000_000,
            },
            current_base_fee: latest_base_fee,
        }
    }

    /// Calculate average gas utilization as a percentage of target.
    fn calculate_avg_utilization(&self, blocks: &[BlockStats]) -> u64 {
        if blocks.is_empty() {
            return 100; // assume at target
        }

        let total_ratio: u64 = blocks
            .iter()
            .map(|b| {
                if b.gas_target == 0 {
                    100
                } else {
                    (b.gas_used * 100) / b.gas_target
                }
            })
            .sum();

        total_ratio / blocks.len() as u64
    }

    /// Get the maximum gas limit for a block (target * elasticity_multiplier).
    pub fn max_gas_limit(&self, gas_target: u64) -> u64 {
        gas_target.saturating_mul(self.elasticity_multiplier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_fee_market() {
        let fm = FeeMarket::default();
        assert_eq!(fm.base_fee, 1_000_000_000);
        assert_eq!(fm.elasticity_multiplier, 2);
        assert_eq!(fm.base_fee_change_denominator, 8);
    }

    #[test]
    fn test_base_fee_no_change_at_target() {
        let mut fm = FeeMarket::default();
        let new_fee = fm.update_base_fee(10_000_000, 10_000_000, 1_000_000_000);
        assert_eq!(new_fee, 1_000_000_000);
    }

    #[test]
    fn test_base_fee_increases_above_target() {
        let mut fm = FeeMarket::default();
        let parent_base_fee = 1_000_000_000;
        let gas_target = 10_000_000;
        let gas_used = 15_000_000;

        let new_fee = fm.update_base_fee(gas_used, gas_target, parent_base_fee);

        let expected = parent_base_fee + 62_500_000;
        assert_eq!(new_fee, expected);
        assert!(new_fee > parent_base_fee);
    }

    #[test]
    fn test_base_fee_decreases_below_target() {
        let mut fm = FeeMarket::default();
        let parent_base_fee = 1_000_000_000;
        let gas_target = 10_000_000;
        let gas_used = 5_000_000;

        let new_fee = fm.update_base_fee(gas_used, gas_target, parent_base_fee);

        let expected = parent_base_fee - 62_500_000;
        assert_eq!(new_fee, expected);
        assert!(new_fee < parent_base_fee);
    }

    #[test]
    fn test_base_fee_clamps_to_minimum() {
        let mut fm = FeeMarket::new(1_000_000, 1_000_000, 500_000_000_000);
        let new_fee = fm.update_base_fee(0, 10_000_000, 1_000_000);
        assert_eq!(new_fee, fm.min_base_fee);
    }

    #[test]
    fn test_base_fee_clamps_to_maximum() {
        let mut fm = FeeMarket::new(500_000_000_000, 1_000_000, 500_000_000_000);
        let new_fee = fm.update_base_fee(20_000_000, 10_000_000, 500_000_000_000);
        assert_eq!(new_fee, fm.max_base_fee);
    }

    #[test]
    fn test_base_fee_empty_block_decreases() {
        let mut fm = FeeMarket::default();
        let parent_base_fee = 1_000_000_000;
        let new_fee = fm.update_base_fee(0, 10_000_000, parent_base_fee);
        let expected = parent_base_fee - parent_base_fee / 8;
        assert_eq!(new_fee, expected);
    }

    #[test]
    fn test_base_fee_full_block_increases() {
        let mut fm = FeeMarket::default();
        let parent_base_fee = 1_000_000_000;
        let gas_target = 10_000_000;
        let gas_used = gas_target * 2;
        let new_fee = fm.update_base_fee(gas_used, gas_target, parent_base_fee);
        let expected = parent_base_fee + parent_base_fee / 8;
        assert_eq!(new_fee, expected);
    }

    #[test]
    fn test_base_fee_zero_target() {
        let mut fm = FeeMarket::default();
        let new_fee = fm.update_base_fee(1000, 0, 1_000_000_000);
        assert_eq!(new_fee, 1_000_000_000);
    }

    #[test]
    fn test_effective_fee_calculation() {
        let fm = FeeMarket::default();
        let result = fm.calculate_effective_fee(100, 10, 50);
        assert!(result.is_some());

        let fee = result.unwrap();
        assert_eq!(fee.base_fee, 50);
        assert_eq!(fee.priority_fee, 10);
        assert_eq!(fee.effective_gas_price, 60);
    }

    #[test]
    fn test_effective_fee_priority_capped() {
        let fm = FeeMarket::default();
        let result = fm.calculate_effective_fee(100, 50, 70);
        assert!(result.is_some());

        let fee = result.unwrap();
        assert_eq!(fee.base_fee, 70);
        assert_eq!(fee.priority_fee, 30);
        assert_eq!(fee.effective_gas_price, 100);
    }

    #[test]
    fn test_effective_fee_insufficient_max_fee() {
        let fm = FeeMarket::default();
        let result = fm.calculate_effective_fee(40, 10, 50);
        assert!(result.is_none());
    }

    #[test]
    fn test_effective_fee_exact_base_fee() {
        let fm = FeeMarket::default();
        let result = fm.calculate_effective_fee(50, 10, 50);
        assert!(result.is_some());

        let fee = result.unwrap();
        assert_eq!(fee.priority_fee, 0);
        assert_eq!(fee.effective_gas_price, 50);
    }

    #[test]
    fn test_fee_burn_calculation() {
        let fm = FeeMarket::default();
        let burn = fm.calculate_fee_burn(1_000_000_000, 21_000);
        assert_eq!(burn, 21_000_000_000_000);
    }

    #[test]
    fn test_fee_burn_zero_gas() {
        let fm = FeeMarket::default();
        assert_eq!(fm.calculate_fee_burn(1_000_000_000, 0), 0);
    }

    #[test]
    fn test_fee_burn_zero_base_fee() {
        let fm = FeeMarket::default();
        assert_eq!(fm.calculate_fee_burn(0, 21_000), 0);
    }

    #[test]
    fn test_fee_estimate_empty_history() {
        let fm = FeeMarket::default();
        let estimate = fm.estimate_fee(&[]);

        assert_eq!(estimate.current_base_fee, fm.base_fee);
        assert!(estimate.low.max_fee_per_gas <= estimate.medium.max_fee_per_gas);
        assert!(estimate.medium.max_fee_per_gas <= estimate.high.max_fee_per_gas);
    }

    #[test]
    fn test_fee_estimate_with_history() {
        let fm = FeeMarket::default();
        let blocks = vec![
            BlockStats {
                gas_used: 12_000_000,
                gas_target: 10_000_000,
                base_fee: 1_000_000_000,
                block_number: 1,
            },
            BlockStats {
                gas_used: 14_000_000,
                gas_target: 10_000_000,
                base_fee: 1_050_000_000,
                block_number: 2,
            },
        ];

        let estimate = fm.estimate_fee(&blocks);

        assert!(estimate.low.max_fee_per_gas > 0);
        assert!(estimate.medium.max_fee_per_gas > estimate.low.max_fee_per_gas);
        assert!(estimate.high.max_fee_per_gas > estimate.medium.max_fee_per_gas);
        assert!(estimate.low.max_priority_fee_per_gas <= estimate.medium.max_priority_fee_per_gas);
    }

    #[test]
    fn test_max_gas_limit() {
        let fm = FeeMarket::default();
        assert_eq!(fm.max_gas_limit(10_000_000), 20_000_000);
    }

    #[test]
    fn test_base_fee_gradual_convergence() {
        let mut fm = FeeMarket::new(1_000_000_000, 1_000_000, 500_000_000_000);
        let gas_target = 10_000_000;
        let gas_used = 5_000_000;

        let mut current_fee = fm.base_fee;
        for _ in 0..10 {
            current_fee = fm.update_base_fee(gas_used, gas_target, current_fee);
        }

        assert!(current_fee < 1_000_000_000);
    }

    #[test]
    fn test_transaction_type_variants() {
        let variants = [
            TransactionType::Legacy,
            TransactionType::Eip2930,
            TransactionType::Eip1559,
        ];

        assert_eq!(variants.len(), 3);
    }
}
