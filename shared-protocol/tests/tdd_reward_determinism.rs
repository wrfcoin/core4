// TDD verification for PR #994: integer reward determinism.
//
// Exercises `TokenEmissionManager` methods and `adjusted_block_reward` at
// various inputs, running each 1000 times and asserting identical results
// on every iteration.  Any floating-point non-determinism or rounding
// instability will cause a failure.
//
// Run: cargo test -p wrfcoin-shared-protocol --test tdd_reward_determinism -- --nocapture

use wrfcoin_shared_protocol::common::constants::{BLOCK_TIME_PROGRESSION, WRF_UNITS_PER_COIN};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    EMISSION_REDUCTION_PERIOD_SECONDS, GENESIS_BLOCK_TIME, INITIAL_BLOCK_REWARD, MAX_SUPPLY,
    adjusted_block_reward, base_block_reward, elapsed_seconds_at_height,
    emission_periods_at_height, total_scheduled_emission,
};
use wrfcoin_shared_protocol::rewards::emission::TokenEmissionManager;

const ITERATIONS: usize = 1_000;

// ---------------------------------------------------------------------------
// Helper: run a closure ITERATIONS times and assert every result equals the
// first.
// ---------------------------------------------------------------------------
fn assert_deterministic<T: Eq + std::fmt::Debug>(label: &str, f: impl Fn() -> T) {
    let first = f();
    for i in 1..ITERATIONS {
        let result = f();
        assert_eq!(
            result, first,
            "{label}: iteration {i} produced a different result"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 1: calculate_contribution_reward determinism
// ---------------------------------------------------------------------------
#[test]
fn contribution_reward_deterministic_across_inputs() {
    let manager = TokenEmissionManager::default();

    // (consensus_reward, contribution_score, oracle_quality, dao_emission_rate)
    let inputs: Vec<(u64, f64, f64, Option<u64>)> = vec![
        (50, 10.0, 1.0, None),
        (50, 5.5, 0.75, Some(100)),
        (1, 0.1, 0.01, None),
        (1_000_000, 9.99, 0.999, Some(500_000)),
        (0, 7.3, 0.5, None),
    ];

    for (cr, cs, oq, dao) in &inputs {
        let cr = *cr;
        let cs = *cs;
        let oq = *oq;
        let dao = *dao;
        assert_deterministic(
            &format!("contribution_reward({cr}, {cs}, {oq}, {dao:?})"),
            || manager.calculate_contribution_reward(cr, cs, oq, dao),
        );
    }
}

// ---------------------------------------------------------------------------
// Test 2: calculate_forecast_skill_reward determinism
// ---------------------------------------------------------------------------
#[test]
fn forecast_skill_reward_deterministic_across_inputs() {
    let manager = TokenEmissionManager::default();

    let inputs: Vec<(u64, f64, f64, Option<u64>)> = vec![
        (50, 1.0, 1.0, None),
        (50, 0.0, 1.0, None),
        (200, 0.5, 0.5, Some(150)),
        (1, 0.333, 0.777, None),
        (999_999, 0.999, 0.001, Some(1_000_000)),
    ];

    for (cr, fs, oq, dao) in &inputs {
        let cr = *cr;
        let fs = *fs;
        let oq = *oq;
        let dao = *dao;
        assert_deterministic(
            &format!("forecast_skill_reward({cr}, {fs}, {oq}, {dao:?})"),
            || manager.calculate_forecast_skill_reward(cr, fs, oq, dao),
        );
    }
}

// ---------------------------------------------------------------------------
// Test 3: distribute_role_rewards determinism
// ---------------------------------------------------------------------------
#[test]
fn distribute_role_rewards_deterministic() {
    let manager = TokenEmissionManager::default();

    let totals = [0u64, 1, 100, 999, 10_000, 1_000_000, u64::MAX / 2];

    for total in totals {
        assert_deterministic(&format!("distribute_role_rewards({total})"), || {
            let map = manager.distribute_role_rewards(total);
            // Convert to sorted vec for stable comparison
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by_key(|(role, _)| role.to_str().to_string());
            entries
        });
    }
}

// ---------------------------------------------------------------------------
// Test 4: distribute_role_rewards never exceeds total
// ---------------------------------------------------------------------------
#[test]
fn distribute_role_rewards_sum_never_exceeds_total() {
    let manager = TokenEmissionManager::default();

    let totals = [0u64, 1, 3, 7, 99, 100, 999, 10_000, 1_000_000];

    for total in totals {
        let map = manager.distribute_role_rewards(total);
        let sum: u64 = map.values().sum();
        assert!(
            sum <= total,
            "distribute_role_rewards({total}): sum {sum} exceeds total"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 5: adjusted_block_reward at halving boundaries
// ---------------------------------------------------------------------------
#[test]
fn adjusted_block_reward_deterministic_at_halving_boundaries() {
    let heights = [0u64, 209_999, 210_000, 420_000, 1_000_000, 2_100_000];

    for height in heights {
        assert_deterministic(&format!("adjusted_block_reward({height})"), || {
            adjusted_block_reward(height)
        });
    }
}

// ---------------------------------------------------------------------------
// Test 6: adjusted_block_reward halving behaviour
// ---------------------------------------------------------------------------
#[test]
fn adjusted_block_reward_halves_at_boundaries() {
    // Before first halving
    let reward_pre = adjusted_block_reward(209_999);
    // At first halving
    let reward_at = adjusted_block_reward(210_000);

    // base_block_reward halves: 50 -> 25.  adjusted_block_reward also factors
    // in compute_target_block_time which may change at different heights, but
    // the base reward definitely drops.  Verify monotonic decrease.
    assert!(
        reward_at <= reward_pre,
        "reward at halving ({reward_at}) should be <= reward before halving ({reward_pre})"
    );

    // Second halving
    let reward_second = adjusted_block_reward(420_000);
    assert!(
        reward_second <= reward_at,
        "reward at second halving ({reward_second}) should be <= reward at first ({reward_at})"
    );

    // Height 10M is NO LONGER exhausted (#2492). Under the block-count schedule
    // this asserted 0 — 47 halvings — which is the same defect as the #2485
    // zero-cliff seen from the other end: block time compresses 14_400x, so a
    // fixed block interval made reductions arrive geometrically faster in
    // wall-clock terms and emission died at year 9.2.
    //
    // With the period denominated in wall-clock seconds, height 10M is only
    // 308_200_000 s in — about 9.8 years, two reductions — so it must still mint.
    // Emission now terminates at ~243.6 years (61 reductions), not year 9.2.
    let reward_deep = adjusted_block_reward(10_000_000);
    assert_eq!(
        reward_deep, 2_469_135,
        "height 10M is ~9.8 years in and must still mint (#2492)"
    );
    assert_eq!(
        emission_periods_at_height(10_000_000),
        2,
        "height 10M is two reduction periods in, not 47"
    );
    assert_eq!(
        elapsed_seconds_at_height(10_000_000),
        308_200_000,
        "elapsed seconds must come from the block-time table, not timestamps"
    );
}

// ---------------------------------------------------------------------------
// Test 7: adjusted_block_reward at block-time progression tier boundaries
//
// The BLOCK_TIME_PROGRESSION table changes target_block_time at specific
// heights, which proportionally changes adjusted_block_reward via:
//   adjusted = base_reward * target_block_time / GENESIS_BLOCK_TIME
//
// node host archive evidence: reward_total=50 for heights 1-999,
//                          reward_total=25 for heights 1000-1200.
// ---------------------------------------------------------------------------
#[test]
fn adjusted_block_reward_matches_block_time_progression_tiers() {
    // All values are BASE UNITS (#2485). Before that fix `INITIAL_BLOCK_REWARD`
    // was the bare literal `50`, so these assertions read 50/25/12/2/0 — the
    // whole-coin figures from the doc comment, being minted as base units.
    //
    // Tier 0: heights 0-999, block_time=14400, reward = 8e10 * 14400/14400
    const TIER0: u64 = 800 * WRF_UNITS_PER_COIN; // 80_000_000_000 = 800 WRF
    assert_eq!(adjusted_block_reward(0), TIER0, "genesis height reward");
    assert_eq!(adjusted_block_reward(1), TIER0, "height 1 reward");
    assert_eq!(adjusted_block_reward(999), TIER0, "last block before first tier transition");

    // Tier 1: heights 1000-4999, block_time=7200, reward = 8e10 * 7200/14400
    const TIER1: u64 = 400 * WRF_UNITS_PER_COIN; // 40_000_000_000 = 400 WRF
    assert_eq!(adjusted_block_reward(1_000), TIER1, "first tier transition at height 1000");
    assert_eq!(adjusted_block_reward(1_200), TIER1, "mid-tier-1");
    assert_eq!(adjusted_block_reward(4_999), TIER1, "last block before second tier transition");

    // Tier 2: heights 5000-24999, block_time=3600, reward = 8e10 * 3600/14400
    const TIER2: u64 = 200 * WRF_UNITS_PER_COIN; // 20_000_000_000 = 200 WRF
    assert_eq!(adjusted_block_reward(5_000), TIER2, "second tier transition at height 5000");
    assert_eq!(adjusted_block_reward(24_999), TIER2, "last block before third tier transition");

    // Tier 3: heights 25000-99999, block_time=600. NOTE this tier straddles the
    // first reduction, which lands at height 43_000 (#2492) — reduction
    // boundaries are wall-clock now, so they no longer align to tier edges.
    // Before the reduction: 8e10 * 600 / 14_400 = 3_333_333_333 (33.33… WRF).
    const TIER3_PRE: u64 = 3_333_333_333;
    assert_eq!(adjusted_block_reward(25_000), TIER3_PRE, "third tier, before the first reduction");
    assert_eq!(adjusted_block_reward(42_999), TIER3_PRE, "last block before the first reduction");

    // After it: base is 8e10 * 2/3 = 53_333_333_333, so 53_333_333_333 * 600 / 14_400.
    const TIER3_POST: u64 = 2_222_222_222;
    assert_eq!(adjusted_block_reward(43_000), TIER3_POST, "first reduction lands at height 43_000");
    assert_eq!(adjusted_block_reward(99_999), TIER3_POST, "still one reduction in");
    assert_eq!(emission_periods_at_height(42_999), 0, "period 0 runs to height 42_999");
    assert_eq!(emission_periods_at_height(43_000), 1, "period 1 starts at height 43_000");

    // Tier 4: block_time=120. THIS IS THE #2485 REGRESSION POINT: it was 0 before
    // the rescale (`50 * 120 / 14_400 = 6000/14_400 -> 0`), which is where the
    // chain stopped minting permanently. It must be non-zero.
    const TIER4: u64 = 444_444_444; // 4.444… WRF
    assert_eq!(adjusted_block_reward(100_000), TIER4, "fourth tier must still mint (#2485)");
    assert_ne!(adjusted_block_reward(100_000), 0, "the 100k zero-cliff must not return");

    // Historical note, deliberately NOT an assertion: the node host archive shows
    // `reward_total=50` for heights 1-999 and `25` for 1000+. Those are the
    // *base-unit* values the defective build minted — 0.0000005 WRF and
    // 0.00000025 WRF — not 50 and 25 WRF. The archive is evidence that #2485 was
    // live in production, so it must not be used to pin expected values here.
}

// ---------------------------------------------------------------------------
// Test 8 (REPLACED by #2485): the permanent guards against re-introducing a
// coin-denominated reward constant.
//
// This test previously asserted the OPPOSITE — that `adjusted_block_reward` is
// zero at height 100_000 and every faster tier — under the heading "documents
// the permanent zero-emission invariant". There is no such invariant. That was
// the #2485 defect (`INITIAL_BLOCK_REWARD` written as a bare `50` and minted as
// base units) pinned as expected behaviour, which is why the bug survived #809,
// #1848 and #1723 untouched: the regression test asserted it.
//
// These three guards are the reason this class cannot recur silently. Each is a
// red control for the constant: revert `INITIAL_BLOCK_REWARD` to a bare `50` and
// each one fails.
// ---------------------------------------------------------------------------

/// Guard 1 — the constant is denominated in base units, not coins.
#[test]
fn emission_constants_are_denominated_in_base_units_2485() {
    assert!(
        INITIAL_BLOCK_REWARD >= WRF_UNITS_PER_COIN,
        "INITIAL_BLOCK_REWARD ({INITIAL_BLOCK_REWARD}) is smaller than one whole \
         coin ({WRF_UNITS_PER_COIN}). Every currency quantity in this workspace \
         is BASE UNITS: a reward meant to be N WRF must be written \
         `N * WRF_UNITS_PER_COIN`. A bare `50` here mints 0.0000005 WRF and, \
         because the block-time normalisation then divides by up to 14_400, \
         truncates emission to zero from height 100_000 onward (#2485)."
    );
    assert_eq!(
        INITIAL_BLOCK_REWARD,
        800 * WRF_UNITS_PER_COIN,
        "genesis block reward must be exactly 800 WRF expressed in base units (#2492)"
    );
}

/// Guard 2 — the schedule's own total lands on `MAX_SUPPLY`.
///
/// `sum((1/2)^k) = 2`, so `50 WRF × 210_000 × 2 = 21_000_000 WRF`. This ties the
/// three constants together, so changing any one of them in isolation fails here.
#[test]
fn the_schedule_total_equals_max_supply_2485() {
    // #2492: the period is wall-clock seconds, so per-period emission is
    // `R / GENESIS_BLOCK_TIME x PERIOD_SECONDS` — block-time independent — and
    // `sum((2/3)^k) = 3`.
    let per_period = INITIAL_BLOCK_REWARD as u128 * EMISSION_REDUCTION_PERIOD_SECONDS as u128
        / GENESIS_BLOCK_TIME as u128;
    let analytic = per_period * 3;
    assert_eq!(
        per_period,
        7_000_000 * WRF_UNITS_PER_COIN as u128,
        "each period must emit exactly 7,000,000 WRF; got {per_period} base units"
    );
    assert_eq!(
        analytic, MAX_SUPPLY,
        "R / T x P x 3 must equal MAX_SUPPLY: {INITIAL_BLOCK_REWARD} / {GENESIS_BLOCK_TIME} \
         x {EMISSION_REDUCTION_PERIOD_SECONDS} x 3 = {analytic}, MAX_SUPPLY = {MAX_SUPPLY}"
    );

    // The walked schedule truncates on each halving, so it lands just under the
    // analytic figure. Pin the gap rather than allowing it to drift: at this
    // denomination the loss is a rounding error, where at the old one it was 10%.
    let walked = total_scheduled_emission();
    assert!(
        walked <= MAX_SUPPLY,
        "walked schedule {walked} must not exceed MAX_SUPPLY {MAX_SUPPLY}"
    );
    let shortfall = MAX_SUPPLY - walked;
    assert!(
        shortfall < WRF_UNITS_PER_COIN as u128 * 100,
        "halving truncation should cost well under 100 WRF out of 21M; lost \
         {shortfall} base units. A large shortfall means the reward is \
         under-denominated and the shifts are eating whole percentage points."
    );
}

/// Guard 2b — the multiply-before-divide intermediate cannot overflow.
///
/// `adjusted_block_reward` computes `base * block_time / GENESIS_BLOCK_TIME`,
/// multiplying first to keep the numerator exact. Rescaling the reward by 10^8
/// (#2485) raised that intermediate by the same factor, so the headroom claim
/// has to be checked rather than asserted in a comment.
///
/// This test exists because the PR body for #2485 named a guard of this kind
/// that had not been written. A stated guarantee with no executable behind it is
/// the same defect class as a test that pins a bug — it reads as covered.
#[test]
fn the_widest_reward_intermediate_cannot_overflow_2485() {
    // The widest possible intermediate: the largest base reward the schedule
    // ever produces, times the largest block time in the table.
    let widest_block_time = BLOCK_TIME_PROGRESSION
        .iter()
        .map(|&(_, t)| t)
        .max()
        .expect("block-time table is non-empty");
    assert_eq!(
        widest_block_time, GENESIS_BLOCK_TIME,
        "genesis is assumed to be the slowest block time; if that changes the \
         bound below must be re-derived"
    );

    let widest = INITIAL_BLOCK_REWARD
        .checked_mul(widest_block_time)
        .expect("INITIAL_BLOCK_REWARD * GENESIS_BLOCK_TIME must not overflow u64");

    // Deliberately NOT hard-coded. #2492 raises the genesis reward from 50 WRF
    // to 800 WRF, which moves this intermediate from 7.2e13 to 1.152e15 and the
    // headroom from ~256_000x to ~16_000x. A literal here would pass on this
    // branch and fail the merged tree — the guard must hold at both magnitudes,
    // which is the whole point of expressing it against the constants.
    let headroom = u64::MAX / widest;
    assert!(
        headroom > 10_000,
        "only {headroom}x headroom between the widest reward intermediate \
         ({widest} = {INITIAL_BLOCK_REWARD} * {widest_block_time}) and u64::MAX. \
         Below this, `base * block_time` in `adjusted_block_reward` needs u128 \
         rather than a comment claiming it is safe."
    );

    // And empirically: no height anywhere in the schedule produces an
    // intermediate that would wrap, including the far end of the range.
    let mut probes: Vec<u64> = BLOCK_TIME_PROGRESSION.iter().map(|&(h, _)| h).collect();
    // #2492: no HALVING_INTERVAL any more — probe around the first wall-clock
    // reduction and out to the far end of the range instead.
    probes.extend([42_999, 43_000, 10_000_000, 1u64 << 40, u64::MAX / 2, u64::MAX]);
    for height in probes {
        let base = base_block_reward(height);
        let block_time = BLOCK_TIME_PROGRESSION
            .iter()
            .rev()
            .find(|&&(start, _)| height >= start)
            .map(|&(_, t)| t)
            .unwrap_or(GENESIS_BLOCK_TIME);
        assert!(
            base.checked_mul(block_time).is_some(),
            "base_block_reward({height}) = {base} times block_time {block_time} \
             overflows u64 — `adjusted_block_reward` would wrap and mint a \
             wrong amount"
        );
        // The public function must agree and never exceed the base reward.
        assert!(
            adjusted_block_reward(height) <= base,
            "adjusted reward at {height} exceeds its base reward — the \
             normalisation factor must be <= 1"
        );
    }
}

/// Guard 3 — the block-time normalisation must not zero a full base reward.
///
/// Scoped to heights inside the FIRST reduction period, where the base reward is
/// still `INITIAL_BLOCK_REWARD`. There, any zero is attributable to the
/// `block_time / GENESIS_BLOCK_TIME` division alone — which is exactly the #2485
/// failure at height 100_000.
///
/// Deliberately not extended to every tier: deep into the schedule the base
/// reward is legitimately small and the division does floor it to zero. That is
/// the schedule terminating, not a denomination defect, and asserting non-zero
/// there would be wrong.
#[test]
fn block_time_scaling_never_zeroes_a_full_base_reward_2485() {
    for &(height, block_time) in BLOCK_TIME_PROGRESSION.iter() {
        if emission_periods_at_height(height) > 0 {
            continue;
        }
        assert_eq!(
            base_block_reward(height),
            INITIAL_BLOCK_REWARD,
            "height {height} should still be in the first halving period"
        );
        assert!(
            adjusted_block_reward(height) > 0,
            "block-time normalisation zeroed a FULL base reward at height \
             {height} (block_time={block_time}): {INITIAL_BLOCK_REWARD} * \
             {block_time} / 14400 truncated to 0. The reward is denominated too \
             small to survive the division — this is #2485."
        );
    }
}

// ---------------------------------------------------------------------------
// Test 9: distribute_role_rewards produces all-zero values when total is 0.
//
// Existing test 3 checks determinism for total=0, and test 4 checks
// sum <= total. Neither pins individual role values. This documents that
// each role receives exactly 0 when there is nothing to distribute.
// ---------------------------------------------------------------------------
#[test]
fn distribute_role_rewards_all_zero_when_total_zero() {
    let manager = TokenEmissionManager::default();
    let map = manager.distribute_role_rewards(0);

    for (role, value) in &map {
        assert_eq!(
            *value, 0,
            "role {:?} must receive 0 when total is 0, got {value}",
            role.to_str()
        );
    }

    // Sanity: the map is non-empty (roles exist)
    assert!(
        !map.is_empty(),
        "distribute_role_rewards should return at least one role"
    );
}

// ---------------------------------------------------------------------------
// Test 10: Zero-reward determinism at block-time tier boundaries.
//
// Runs adjusted_block_reward 1000x at each zero-reward tier boundary
// and asserts identical results every iteration. Proves no floating-point
// or rounding instability at the zero boundary.
// ---------------------------------------------------------------------------
#[test]
fn zero_reward_deterministic_at_block_time_tiers() {
    let zero_tier_heights = [100_000u64, 500_000, 1_000_000, 5_000_000, 10_000_000];

    for height in zero_tier_heights {
        assert_deterministic(&format!("adjusted_block_reward({height})"), || {
            adjusted_block_reward(height)
        });
    }
}

// ---------------------------------------------------------------------------
// Test 11: apply_adjustments is deterministic with integer math (#1189).
//
// Exercises the reward adjustment path that previously used f64 multiplication
// (base * rw * accuracy_factor * reputation_factor). Now uses u128 integer
// arithmetic. Runs 1000 iterations with non-trivial inputs (region boost,
// non-neutral accuracy/reputation) and asserts identical results every time.
// ---------------------------------------------------------------------------
#[test]
fn apply_adjustments_integer_determinism() {
    let mut manager = TokenEmissionManager::default();

    // Set non-trivial config: 2x region boost, 1.25 accuracy multiplier, 0.9 rep multiplier
    let mut weights = rustc_hash::FxHashMap::default();
    weights.insert("desert-region".into(), 2.0);
    manager.set_region_weights(weights);
    manager.set_accuracy_multiplier(1.25);
    manager.set_reputation_multiplier(0.9);

    // Non-trivial inputs: accuracy=0.8, reputation=0.6
    let cases: Vec<(u64, Option<&str>, Option<f64>, Option<f64>)> = vec![
        (1_000_000, Some("desert-region"), Some(0.8), Some(0.6)),
        (500, None, Some(0.0), Some(1.0)),
        (100, Some("unknown-region"), Some(0.5), Some(0.5)),
        (999_999, Some("desert-region"), Some(1.0), Some(0.0)),
        (0, Some("desert-region"), Some(0.8), Some(0.8)),
    ];

    for (base, region, accuracy, reputation) in &cases {
        let base = *base;
        let region = *region;
        let accuracy = *accuracy;
        let reputation = *reputation;
        assert_deterministic(
            &format!("apply_adjustments({base}, {region:?}, {accuracy:?}, {reputation:?})"),
            || manager.apply_adjustments(base, region, accuracy, reputation),
        );
    }
}

// ---------------------------------------------------------------------------
// Test 12: Halving schedule total supply is finite and deterministic (#1189).
//
// Sums adjusted_block_reward across all block heights from 0 to 10M.
// The total must be identical across runs and finite (no infinite emission).
// This verifies the halving + block-time-progression schedule produces the
// expected total supply without any floating-point accumulation errors.
// ---------------------------------------------------------------------------
#[test]
fn halving_schedule_total_supply_is_finite_and_deterministic() {
    // Segment boundaries are the union of block-time changes AND halving
    // boundaries (#2485).
    //
    // The previous hand-written table listed block-time tiers only, and asserted
    // the reward is constant within each. That silently stopped being true once
    // rewards were non-zero, because a reduction boundary can fall *inside* a
    // block-time tier. It went unnoticed because every reward in that range was 0
    // before #2485, so "constant within the tier" held vacuously. Deriving the
    // boundaries instead of listing them is what stops the table going stale.
    //
    // #2492: reduction boundaries are now wall-clock, so their heights are found
    // by scanning for the period counter changing rather than by multiplying a
    // fixed block interval.
    const CEILING: u64 = 10_000_000;
    let mut edges: Vec<u64> = BLOCK_TIME_PROGRESSION.iter().map(|&(h, _)| h).collect();
    let mut previous_period = emission_periods_at_height(0);
    for height in 0..=CEILING {
        let period = emission_periods_at_height(height);
        if period != previous_period {
            edges.push(height);
            previous_period = period;
        }
    }
    edges.retain(|&h| h <= CEILING);
    edges.push(CEILING + 1);
    edges.sort_unstable();
    edges.dedup();
    let tiers: Vec<(u64, u64)> = edges
        .windows(2)
        .map(|w| (w[0], w[1] - 1))
        .collect();

    let mut total_supply: u128 = 0;

    for (start, end) in &tiers {
        let reward_at_start = adjusted_block_reward(*start);
        let reward_at_end = adjusted_block_reward(*end);
        // Within a tier the reward is constant (no halving within these ranges).
        assert_eq!(
            reward_at_start, reward_at_end,
            "reward should be constant within tier [{start}, {end}]"
        );
        let blocks = end - start + 1;
        total_supply += reward_at_start as u128 * blocks as u128;
    }

    // Total supply must be finite and non-zero.
    assert!(total_supply > 0, "total supply should be positive");
    // Verify determinism: compute again and compare.
    let mut total_supply_2: u128 = 0;
    for (start, end) in &tiers {
        let reward = adjusted_block_reward(*start);
        let blocks = end - start + 1;
        total_supply_2 += reward as u128 * blocks as u128;
    }
    assert_eq!(
        total_supply, total_supply_2,
        "total supply calculation must be deterministic"
    );

    // Expected total across heights 0..=10_000_000, in base units (#2485).
    //
    // The previous pinned figure was 540_000 base units — 0.0054 WRF for the
    // whole chain — and that number was correct for the defective build. It is
    // the single clearest artifact of #2485 in the test suite: a supply test
    // asserting the chain mints half a cent, against a 21_000_000 WRF cap, and
    // passing.
    // #2492 reshaped the schedule to wall-clock periods, so this figure moved
    // from 66_217_445_115_000 (662_174 WRF over 10M blocks, block-count periods)
    // to the value below — 13_054_320 WRF over the same span, which is ~9.8 years
    // and 62% of the 21M cap. Front-loaded by design: the block-time progression
    // compresses 14_400x, so most wall-clock time (and therefore most emission)
    // sits in the early, slow-block part of the height range.
    const EXPECTED_TOTAL: u128 = 1_305_432_099_777_135; // 13_054_320.99 WRF
    assert_eq!(
        total_supply, EXPECTED_TOTAL,
        "realised emission over the first 10M blocks changed; if this is \
         intentional, update the figure and say why in the PR"
    );

    // Sanity band rather than a bare equality: realised emission is well under
    // MAX_SUPPLY because the block-time factor is <= 1 while the halving schedule
    // assumes full base rewards (see `adjusted_block_reward`). Assert it is a
    // sane fraction — not zero, not over the cap.
    assert!(
        total_supply < MAX_SUPPLY,
        "realised emission {total_supply} must stay under MAX_SUPPLY {MAX_SUPPLY}"
    );
    assert!(
        total_supply > WRF_UNITS_PER_COIN as u128 * 1_000,
        "realised emission over 10M blocks is {total_supply} base units — under \
         1000 WRF. That is the #2485 shape: rewards truncating to near-nothing."
    );
}
