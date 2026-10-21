use std::sync::Arc;

use proptest::prelude::*;
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::common::constants::BLOCK_TIME_PROGRESSION;
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    emission_periods_at_height,
    MAX_SUPPLY, ProofOfPublishing, adjusted_block_reward, base_block_reward,
    commit_window_blocks, compute_target_block_time, max_feeless_per_worker_per_epoch,
    reveal_window_blocks,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

/// First height strictly after `height` at which the reduction counter advances.
///
/// #2492: reductions are denominated in wall-clock seconds, not blocks, so the
/// boundary is no longer `((h / 210_000) + 1) * 210_000`. Block time compresses
/// 14_400x across `BLOCK_TIME_PROGRESSION`, so the block distance between two
/// reductions is not constant and has to be searched for rather than computed.
fn next_reduction_boundary(height: u64) -> Option<u64> {
    let start_period = emission_periods_at_height(height);
    // Exponential probe for a height in the next period, then bisect to the exact
    // boundary. Bounded: the schedule terminates after 61 reductions.
    let mut hi = height.max(1);
    while emission_periods_at_height(hi) == start_period {
        hi = hi.checked_mul(2)?;
        if hi > 1 << 40 {
            return None;
        }
    }
    let mut lo = height;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if emission_periods_at_height(mid) == start_period {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

#[tokio::test]
async fn supply_cap_holds_during_block_production_across_block_time_tiers() {
    // Keep 5_000 base units available so a few blocks can still mint while
    // staying close to the cap.
    let near_cap_seed = MAX_SUPPLY.saturating_sub(5_000);

    for &(activation_height, _) in BLOCK_TIME_PROGRESSION.iter() {
        let mut chain = Blockchain::default();
        chain.height = activation_height.saturating_sub(1);
        chain.state.create_account(addr(7), Balance::new(near_cap_seed));

        let chain = Arc::new(RwLock::new(chain));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let mut pop = ProofOfPublishing::new(1, 60, 1_000_000, 10);
        // #2523: a test chain with no oracle ceremony must SAY so — silence at
        // height 1 is refused, because it cannot be told from a misconfigured cut.
        pop.declare_no_genesis_oracles();

        let mut prev_total = near_cap_seed;
        for _ in 0..3 {
            let _block = pop
                .create_next_block(
                    chain.clone(),
                    mempool.clone(),
                    "0x1111111111111111111111111111111111111111",
                )
                .await
                .expect("block creation must succeed");

            let total = chain.read().await.state.total_minted_supply();
            assert!(
                total <= MAX_SUPPLY,
                "total supply must stay under MAX_SUPPLY at tier height {}: {} > {}",
                activation_height,
                total,
                MAX_SUPPLY
            );
            assert!(
                total >= prev_total,
                "minted supply must be monotonic at tier height {}: {} < {}",
                activation_height,
                total,
                prev_total
            );
            prev_total = total;
        }
    }
}

#[test]
fn reward_curve_monotonicity_and_pre_floor_rate_stability() {
    let mut previous_reward: Option<u64> = None;
    let mut previous_rate_ppm: Option<u128> = None;

    for &(height, block_time) in BLOCK_TIME_PROGRESSION.iter() {
        let reward = adjusted_block_reward(height);

        if let Some(prev) = previous_reward {
            assert!(
                reward <= prev,
                "adjusted reward must not increase as block time decreases: h={} r={} prev={}",
                height,
                reward,
                prev
            );
        }

        // Reward-per-second check is meaningful before flooring at 1 WRF/block.
        if reward > 1 {
            let rate_ppm = (reward as u128) * 1_000_000 / (block_time as u128);
            if let Some(prev_rate) = previous_rate_ppm {
                assert!(
                    rate_ppm <= prev_rate,
                    "pre-floor reward/sec must be non-increasing: h={} rate={} prev_rate={}",
                    height,
                    rate_ppm,
                    prev_rate
                );
            }
            previous_rate_ppm = Some(rate_ppm);
        }

        previous_reward = Some(reward);
    }
}

#[test]
fn feeless_epoch_cap_scales_with_block_time_progression() {
    for window in BLOCK_TIME_PROGRESSION.windows(2) {
        let (h_slow, t_slow) = window[0];
        let (h_fast, t_fast) = window[1];
        assert!(t_slow > t_fast, "fixture assumption: block time must decrease");

        let slow_cap = max_feeless_per_worker_per_epoch(h_slow);
        let fast_cap = max_feeless_per_worker_per_epoch(h_fast);

        assert!(
            slow_cap > fast_cap,
            "fee-less per-epoch cap should be higher at slower blocks: h{}={} > h{}={}",
            h_slow,
            slow_cap,
            h_fast,
            fast_cap
        );
    }
}

#[test]
fn commit_reveal_windows_are_wall_clock_bounded_across_tiers() {
    // Bug #884 was 120 days at genesis. The original flat 4h bound here was
    // derived BEFORE #2492 moved genesis block time to 14,400s; production's
    // `window_blocks_capped` deliberately gives the 3-block functional floor
    // precedence over the wall-clock cap (a shorter window would be unusable —
    // a commit+reveal needs blocks to land in), so the two slowest tiers
    // lawfully exceed 4h. Tony accepted the functional minimum on 2026-07-30
    // (#2622): the bound is now tier-aware — the wall-clock cap, except where
    // the 3-block floor exceeds it. This asserts the law production already
    // implements; it is NOT a flat loosening: every tier from h=5,000 on is
    // still held to 4h exactly as before.
    const MAX_WINDOW_SECONDS: u64 = 14_400;
    const FLOOR_BLOCKS: u64 = 3;

    let mut floor_dominated_tiers = 0u32;
    for &(height, _) in BLOCK_TIME_PROGRESSION.iter() {
        let block_time = compute_target_block_time(height);
        let tier_bound = (FLOOR_BLOCKS * block_time).max(MAX_WINDOW_SECONDS);
        if FLOOR_BLOCKS * block_time > MAX_WINDOW_SECONDS {
            floor_dominated_tiers += 1;
        }
        let commit_secs = commit_window_blocks(height).saturating_mul(block_time);
        let reveal_secs = reveal_window_blocks(height).saturating_mul(block_time);

        assert!(
            commit_secs <= tier_bound,
            "commit window too large at h{}: {}s (tier bound {}s)",
            height,
            commit_secs,
            tier_bound
        );
        assert!(
            reveal_secs <= tier_bound,
            "reveal window too large at h{}: {}s (tier bound {}s)",
            height,
            reveal_secs,
            tier_bound
        );
        assert!(
            commit_secs >= 1 && reveal_secs >= 1,
            "window wall-clock durations must stay positive at h{}",
            height
        );
    }

    // Exactly two tiers (h=0 at 12h, h=1,000 at 6h) are floor-dominated
    // today. Pinned so a future BLOCK_TIME_PROGRESSION change that widens
    // floor dominance forces this derivation to be redone consciously
    // instead of inheriting the allowance silently — the #884 failure mode.
    assert_eq!(
        floor_dominated_tiers, 2,
        "floor-dominated tier count changed; re-derive the window bounds \
         (#2622 window-bound decision) before accepting the new progression"
    );
}

#[test]
fn reduction_boundary_is_consistent_in_each_progression_phase() {
    // Renamed from `halving_boundary_...` (#2492): the schedule retains 2/3 at a
    // wall-clock boundary, it does not halve at a block-count one.
    //
    // No `.max(1)` floor (#2485). The old expectation was
    // `(reward_before / 2).max(1)`, asserting the reward can never reach zero —
    // false for any terminating schedule.
    //
    // The coverage count is ASSERTED below, not described in prose (#2485
    // review): an earlier revision claimed "7 of 9" in a comment while the test
    // asserted neither number, so the figure could not be falsified. It was
    // actually 8 of 9 on the block-count schedule.
    let mut asserted = 0usize;
    for &(height, _) in BLOCK_TIME_PROGRESSION.iter() {
        let Some(boundary) = next_reduction_boundary(height) else {
            continue; // schedule terminated past this phase
        };
        let reward_before = base_block_reward(boundary.saturating_sub(1));
        let reward_at = base_block_reward(boundary);
        let expected = reward_before * 2 / 3;

        assert_eq!(
            reward_at, expected,
            "reduction mismatch near phase h{height} at boundary {boundary}: \
             before={reward_before} at={reward_at} expected={expected}"
        );
        // The reduction must be observable, not a no-op. This is the assertion
        // the block-count schedule could never make: emission died at year 9.2,
        // so every boundary past h100_000 compared 0 against 0 (#2492).
        assert!(
            reward_at > 0 && reward_at < reward_before,
            "reduction at {boundary} must be a real step down, not 0 -> 0 \
             (before={reward_before}, at={reward_at})"
        );
        asserted += 1;
    }

    // Exact, not a `>=` floor. A `>=` bound cannot detect the schedule dying
    // earlier, which is the whole failure mode (#2485/#2492): on the block-count
    // schedule this count fell to 8, and before the base-unit rescale to 5.
    assert_eq!(
        asserted,
        BLOCK_TIME_PROGRESSION.len(),
        "expected a live, non-vacuous reduction boundary for every one of the {} \
         progression phases; got {asserted}. A DROP means emission terminates \
         earlier than it should. A RISE means the block-time table grew a phase \
         and this figure needs re-deriving.",
        BLOCK_TIME_PROGRESSION.len()
    );
    assert_eq!(
        BLOCK_TIME_PROGRESSION.len(),
        9,
        "the count above is derived from a 9-phase table"
    );
}

proptest! {
    #[test]
    fn feeless_epoch_cap_matches_block_time_order_for_random_heights(h1 in 0u64..20_000_000, h2 in 0u64..20_000_000) {
        let t1 = compute_target_block_time(h1);
        let t2 = compute_target_block_time(h2);
        let c1 = max_feeless_per_worker_per_epoch(h1);
        let c2 = max_feeless_per_worker_per_epoch(h2);

        if t1 > t2 {
            prop_assert!(c1 >= c2, "slower block time should have >= cap: h1={} t1={} c1={} h2={} t2={} c2={}", h1, t1, c1, h2, t2, c2);
        } else if t1 < t2 {
            prop_assert!(c1 <= c2, "faster block time should have <= cap: h1={} t1={} c1={} h2={} t2={} c2={}", h1, t1, c1, h2, t2, c2);
        } else {
            prop_assert_eq!(c1, c2, "equal block time should yield equal cap: h1={} h2={} t={}", h1, h2, t1);
        }
    }
}
