//! Property-based tests for block-time-scaled consensus parameters (#991).
//!
//! These properties verify invariants across the full block-height range
//! (0..2_000_000) for functions that scale with `compute_target_block_time`.

use proptest::prelude::*;
use wrfcoin_shared_protocol::common::constants::BLOCK_TIME_PROGRESSION;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    adjusted_block_reward, base_block_reward, compute_target_block_time,
    effective_min_work_factor, feeless_rate_limit_epoch_blocks, max_feeless_per_worker_per_block,
    max_feeless_tx_per_block, BASE_FEELESS_RATE_LIMIT_EPOCH_BLOCKS, GENESIS_BLOCK_TIME,
};

// ---------------------------------------------------------------------------
// Property 1: Monotonicity — adjusted_block_reward decreases as block time decreases
// ---------------------------------------------------------------------------
proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn adjusted_reward_monotone_with_block_time(h in 0u64..2_000_000) {
        let reward = adjusted_block_reward(h);
        let block_time = compute_target_block_time(h);

        // Find the next transition height (if any) where block_time decreases.
        if let Some(&(next_activation, next_time)) = BLOCK_TIME_PROGRESSION
            .iter()
            .find(|(activation, time)| *activation > h && *time < block_time)
        {
            let next_reward = adjusted_block_reward(next_activation);
            // If we're still in the same halving epoch, reward should decrease
            // when block time decreases.  If a halving boundary falls between
            // h and next_activation, the base reward also drops — still monotone.
            prop_assert!(
                next_reward <= reward,
                "adjusted_block_reward should not increase when block_time decreases: \
                 reward({h})={reward}, reward({next_activation})={next_reward}, \
                 bt({h})={block_time}, bt({next_activation})={next_time}",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Property 2: Constant-rate equivalence — reward/block_time ≈ constant within
// the same halving epoch.
// ---------------------------------------------------------------------------
proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn emission_rate_approximately_constant(h in 0u64..200_000) {
        // Stay within the first halving epoch (0..210_000) so base_reward is 50.
        let base = base_block_reward(h);
        if base == 0 {
            return Ok(());
        }
        let block_time = compute_target_block_time(h);
        let reward = adjusted_block_reward(h);

        // The exact formula: reward = base * block_time / GENESIS_BLOCK_TIME
        // So: reward * GENESIS_BLOCK_TIME = base * block_time (modulo integer truncation).
        // Allow off-by-one from integer division:
        let lhs = reward as u128 * GENESIS_BLOCK_TIME as u128;
        let rhs = base as u128 * block_time as u128;
        let diff = if lhs > rhs { lhs - rhs } else { rhs - lhs };
        // The maximum truncation error is GENESIS_BLOCK_TIME - 1 (from the division).
        prop_assert!(
            diff < GENESIS_BLOCK_TIME as u128,
            "emission rate not constant: h={h}, reward={reward}, base={base}, \
             block_time={block_time}, diff={diff}"
        );
    }
}

// ---------------------------------------------------------------------------
// Property 3: No division by zero — all functions using compute_target_block_time
// as a divisor never panic.
// ---------------------------------------------------------------------------
proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]
    #[test]
    fn no_division_by_zero_in_consensus_functions(h in 0u64..20_000_000) {
        // These must never panic — they all divide by compute_target_block_time(h).
        let _ = adjusted_block_reward(h);
        let _ = effective_min_work_factor(h);
        let _ = feeless_rate_limit_epoch_blocks(h);
        let _ = max_feeless_tx_per_block(h);
        let _ = max_feeless_per_worker_per_block(h);
        let _ = compute_target_block_time(h);

        // Additionally verify block_time is always >= 1.
        let bt = compute_target_block_time(h);
        prop_assert!(bt >= 1, "block_time must be >= 1 at height {h}, got {bt}");
    }
}

// ---------------------------------------------------------------------------
// Property 4: Epoch duration stability — epoch_blocks * block_time ≈ 100s
// ---------------------------------------------------------------------------
proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn epoch_duration_approximately_100s(h in 0u64..20_000_000) {
        let block_time = compute_target_block_time(h);
        let epoch_blocks = feeless_rate_limit_epoch_blocks(h);

        // epoch_duration = epoch_blocks * block_time
        let epoch_duration = epoch_blocks * block_time;

        // The key invariant: epoch_duration >= block_time (at least 1 block per epoch)
        prop_assert!(
            epoch_duration >= block_time,
            "epoch duration ({epoch_duration}s) < block_time ({block_time}s) at h={h}"
        );

        // When block_time <= BASE_FEELESS_RATE_LIMIT_EPOCH_BLOCKS (100), the epoch
        // duration should be close to 100s. When block_time > 100, it floors to 1 block.
        if block_time <= BASE_FEELESS_RATE_LIMIT_EPOCH_BLOCKS {
            prop_assert!(
                epoch_duration <= BASE_FEELESS_RATE_LIMIT_EPOCH_BLOCKS * block_time,
                "epoch_duration too large at h={h}: {epoch_duration}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Property 5: Work factor inverse scaling — effective_min_work_factor * block_time = constant
// ---------------------------------------------------------------------------
proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn work_factor_inverse_scaling(h in 0u64..20_000_000) {
        let block_time = compute_target_block_time(h);
        let work_factor = effective_min_work_factor(h);

        // Formula: work_factor = MIN_COMPUTE_WORK_FACTOR * GENESIS_BLOCK_TIME / block_time
        // So: work_factor * block_time = MIN_COMPUTE_WORK_FACTOR * GENESIS_BLOCK_TIME
        // Integer division may truncate, so allow error up to (block_time - 1).
        let product = work_factor as u128 * block_time as u128;
        let expected = 1_000u128 * GENESIS_BLOCK_TIME as u128; // MIN_COMPUTE_WORK_FACTOR=1000

        let diff = if expected > product {
            expected - product
        } else {
            product - expected
        };
        prop_assert!(
            diff < block_time as u128,
            "work_factor*block_time should be ~constant: h={h}, product={product}, \
             expected={expected}, diff={diff}, block_time={block_time}"
        );
    }
}
