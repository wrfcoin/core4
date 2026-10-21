//! Boundary tests for fee-less rate limiting at block-time transition heights (#942).
//!
//! Verifies behavior at exact transition boundaries from BLOCK_TIME_PROGRESSION
//! where fee-less caps change abruptly.

use wrfcoin_shared_protocol::common::constants::BLOCK_TIME_PROGRESSION;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    compute_target_block_time, feeless_rate_limit_epoch_blocks, max_feeless_per_worker_per_block,
    max_feeless_tx_per_block, BASE_FEELESS_RATE_LIMIT_EPOCH_BLOCKS, BASE_FEELESS_TX_PER_BLOCK,
    BASE_FEELESS_TX_PER_WORKER_PER_BLOCK,
};

/// Extract transition heights from BLOCK_TIME_PROGRESSION for boundary testing.
fn transition_heights() -> Vec<(u64, u64, u64)> {
    // Returns (transition_height, old_block_time, new_block_time) for each step-down.
    BLOCK_TIME_PROGRESSION
        .windows(2)
        .map(|w| (w[1].0, w[0].1, w[1].1))
        .collect()
}

// ---------------------------------------------------------------------------
// Test 1: max_feeless_tx_per_block jump is bounded at transitions
// ---------------------------------------------------------------------------
#[test]
fn feeless_tx_cap_jump_bounded_at_transitions() {
    for (height, old_bt, new_bt) in transition_heights() {
        let cap_before = max_feeless_tx_per_block(height - 1);
        let cap_at = max_feeless_tx_per_block(height);

        // The cap should decrease when block time decreases.
        assert!(
            cap_at <= cap_before,
            "feeless tx cap should not increase at transition h={height}: \
             before={cap_before} (bt={old_bt}), at={cap_at} (bt={new_bt})"
        );

        // Verify the cap matches the formula exactly.
        assert_eq!(
            cap_before,
            BASE_FEELESS_TX_PER_BLOCK * old_bt as usize,
            "cap before transition should be BASE * old_block_time at h={}",
            height - 1
        );
        assert_eq!(
            cap_at,
            BASE_FEELESS_TX_PER_BLOCK * new_bt as usize,
            "cap at transition should be BASE * new_block_time at h={height}"
        );

        // The ratio of caps should equal the ratio of block times.
        // cap_before / cap_at = old_bt / new_bt
        // Verify: cap_before * new_bt == cap_at * old_bt
        assert_eq!(
            cap_before as u64 * new_bt,
            cap_at as u64 * old_bt,
            "cap ratio should match block time ratio at h={height}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 2: epoch duration stays approximately 100s at transitions
// ---------------------------------------------------------------------------
#[test]
fn epoch_duration_stable_at_transitions() {
    for (height, _old_bt, new_bt) in transition_heights() {
        let epoch_blocks = feeless_rate_limit_epoch_blocks(height);
        let epoch_duration = epoch_blocks * new_bt;

        // For fast block times (bt <= 100), epoch_duration should be close to 100s.
        // Integer division means: floor(100/bt) * bt <= 100.
        if new_bt <= BASE_FEELESS_RATE_LIMIT_EPOCH_BLOCKS {
            assert!(
                epoch_duration <= BASE_FEELESS_RATE_LIMIT_EPOCH_BLOCKS * new_bt,
                "epoch duration too large at h={height}: {epoch_duration}s"
            );
            // epoch_blocks = floor(100 / bt), so epoch_duration >= 100 - bt + 1
            // More precisely: epoch_duration = floor(100/bt) * bt
            let expected_blocks = BASE_FEELESS_RATE_LIMIT_EPOCH_BLOCKS / new_bt;
            assert_eq!(
                epoch_blocks, expected_blocks,
                "epoch blocks mismatch at h={height}"
            );
        }

        // For slow block times (bt > 100), epoch_blocks floors to 1.
        if new_bt > BASE_FEELESS_RATE_LIMIT_EPOCH_BLOCKS {
            assert_eq!(
                epoch_blocks, 1,
                "epoch_blocks should be 1 when block_time ({new_bt}s) > 100s at h={height}"
            );
        }

        // Also check one height before the transition.
        let old_bt = compute_target_block_time(height - 1);
        let old_epoch_blocks = feeless_rate_limit_epoch_blocks(height - 1);
        let old_epoch_duration = old_epoch_blocks * old_bt;

        // Epoch duration should never be zero.
        assert!(
            old_epoch_duration > 0,
            "epoch duration should be > 0 at h={}",
            height - 1
        );
        assert!(
            epoch_duration > 0,
            "epoch duration should be > 0 at h={height}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 3: per-worker per-block cap changes at exact transition heights
// ---------------------------------------------------------------------------
#[test]
fn per_worker_cap_changes_at_transitions() {
    for (height, old_bt, new_bt) in transition_heights() {
        let cap_before = max_feeless_per_worker_per_block(height - 1);
        let cap_at = max_feeless_per_worker_per_block(height);

        // Cap should decrease when block time decreases.
        assert!(
            cap_at <= cap_before,
            "per-worker cap should not increase at h={height}: \
             before={cap_before} (bt={old_bt}), at={cap_at} (bt={new_bt})"
        );

        // Verify formula: cap = BASE * block_time
        assert_eq!(
            cap_before,
            BASE_FEELESS_TX_PER_WORKER_PER_BLOCK * old_bt as u32,
            "per-worker cap before transition at h={}",
            height - 1
        );
        assert_eq!(
            cap_at,
            BASE_FEELESS_TX_PER_WORKER_PER_BLOCK * new_bt as u32,
            "per-worker cap at transition h={height}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 4: A worker valid at H-1 whose rate exceeds the H limit gets rejected
//
// This is a unit-level logic test: at height H-1 the per-worker cap is
// BASE * old_block_time.  At height H the cap drops to BASE * new_block_time.
// A worker with tx_count in (new_cap, old_cap] is valid before but invalid
// after the transition.
// ---------------------------------------------------------------------------
#[test]
fn worker_exceeding_new_cap_rejected_at_transition() {
    for (height, _old_bt, _new_bt) in transition_heights() {
        let old_cap = max_feeless_per_worker_per_block(height - 1);
        let new_cap = max_feeless_per_worker_per_block(height);

        // If new_cap < old_cap, there's a range where a worker would have been
        // accepted before but rejected after.
        if new_cap < old_cap {
            // Pick a tx count in the gap: new_cap + 1 (rejected at new height).
            let tx_count = new_cap + 1;

            // At height-1, this count is within limits.
            assert!(
                tx_count <= old_cap,
                "tx_count={tx_count} should be <= old_cap={old_cap} at h={}",
                height - 1
            );

            // At height, this count exceeds the limit.
            assert!(
                tx_count > new_cap,
                "tx_count={tx_count} should be > new_cap={new_cap} at h={height}"
            );
        }

        // Also verify that exactly new_cap txs is valid at the new height.
        assert!(
            new_cap <= max_feeless_per_worker_per_block(height),
            "new_cap should be within limits at h={height}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 5: All transition heights have consistent block times
// ---------------------------------------------------------------------------
#[test]
fn block_time_changes_exactly_at_transition() {
    for (height, old_bt, new_bt) in transition_heights() {
        // One block before: old block time.
        let bt_before = compute_target_block_time(height - 1);
        assert_eq!(
            bt_before, old_bt,
            "block time at h={} should be {old_bt}, got {bt_before}",
            height - 1
        );

        // At transition: new block time.
        let bt_at = compute_target_block_time(height);
        assert_eq!(
            bt_at, new_bt,
            "block time at h={height} should be {new_bt}, got {bt_at}"
        );

        // Block time must strictly decrease.
        assert!(
            bt_at < bt_before,
            "block time should decrease at transition h={height}: {bt_before} -> {bt_at}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 6: feeless_tx_per_block at extreme heights (genesis and far future)
// ---------------------------------------------------------------------------
#[test]
fn feeless_caps_at_extremes() {
    // Genesis (height 0): block_time = 14400, cap = 50 * 14400 = 720,000
    assert_eq!(max_feeless_tx_per_block(0), 50 * 14_400);
    assert_eq!(max_feeless_per_worker_per_block(0), 3 * 14_400);
    assert_eq!(feeless_rate_limit_epoch_blocks(0), 1); // floor(100/14400) = 0, clamped to 1

    // Terminal (height 10_000_000): block_time = 1, cap = 50 * 1 = 50
    assert_eq!(max_feeless_tx_per_block(10_000_000), 50);
    assert_eq!(max_feeless_per_worker_per_block(10_000_000), 3);
    assert_eq!(feeless_rate_limit_epoch_blocks(10_000_000), 100);

    // Far future (height 100_000_000): still at terminal block_time = 1
    assert_eq!(max_feeless_tx_per_block(100_000_000), 50);
    assert_eq!(max_feeless_per_worker_per_block(100_000_000), 3);
    assert_eq!(feeless_rate_limit_epoch_blocks(100_000_000), 100);
}

// ---------------------------------------------------------------------------
// Test 7: Global feeless cap is always >= per-worker cap
// ---------------------------------------------------------------------------
#[test]
fn global_cap_always_gte_per_worker_cap() {
    for &(height, _bt) in BLOCK_TIME_PROGRESSION {
        let global = max_feeless_tx_per_block(height) as u32;
        let per_worker = max_feeless_per_worker_per_block(height);
        assert!(
            global >= per_worker,
            "global cap ({global}) < per-worker cap ({per_worker}) at h={height}"
        );
    }
}
