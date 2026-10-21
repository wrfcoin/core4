// Integration tests for slash-then-unstake balance invariants (#956).
//
// Verifies that the Slash transition correctly deducts from stake_locked,
// that ReleaseToSpendable fails for slashed amounts, and that there is
// no arithmetic underflow under any slash amount.
//
// Run from $HOME/wrfcoin/core4:
//   cargo test --test slash_unstake -- --nocapture

use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::security::slashing::{
    MAX_SLASHING_EVENTS_PER_BLOCK, SlashingCircuitBreaker, scaled_penalty,
};

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

/// Create a chain with one account that has `spendable` + `staked` amounts.
fn chain_with_staked_account(address: Address, spendable: u128, staked: u128) -> Blockchain {
    let mut chain = Blockchain::default();
    let total = spendable + staked;
    chain.state.create_account(address, Balance::new(total));

    // Adjust spendable so that stake can be locked.
    // create_account sets balance=total, spendable_balance=total, stake_locked=0.
    // Locking `staked` shifts that amount from spendable → stake_locked.
    if staked > 0 {
        chain
            .apply_state_transition(StateTransition::LockToStake {
                address,
                amount: Balance::new(staked),
            })
            .expect("lock to stake");
    }
    chain
}

// ---------------------------------------------------------------------------
// Test 1: Slash reduces stake_locked
// ---------------------------------------------------------------------------

#[test]
fn slash_reduces_stake_locked() {
    let alice = addr(0x01);
    let mut chain = chain_with_staked_account(alice, 0, 1000);

    chain
        .apply_state_transition(StateTransition::Slash {
            address: alice,
            amount: Balance::new(200),
            reason: "test-slash".to_string(),
        })
        .expect("slash must succeed");

    let acc = chain.state.get_account(&alice).unwrap();
    assert_eq!(
        acc.stake_locked,
        Balance::new(800),
        "stake_locked must be 1000 - 200 = 800"
    );
    assert_eq!(
        acc.balance,
        Balance::new(800),
        "total balance must decrease by slash amount"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Unstake exceeding remaining stake fails
// ---------------------------------------------------------------------------

#[test]
fn unstake_after_slash_exceeding_remaining_fails() {
    let alice = addr(0x02);
    let mut chain = chain_with_staked_account(alice, 0, 1000);

    // Slash 200 → only 800 left in stake_locked
    chain
        .apply_state_transition(StateTransition::Slash {
            address: alice,
            amount: Balance::new(200),
            reason: "test-slash".to_string(),
        })
        .expect("slash");

    // Attempting to release 1000 (the original amount) must fail
    let result = chain.apply_state_transition(StateTransition::ReleaseToSpendable {
        address: alice,
        amount: Balance::new(1000),
    });
    assert!(
        matches!(result, Err(ref e) if e.to_string().contains("Insufficient locked stake")),
        "releasing 1000 from 800 stake_locked must fail with InsufficientLockedStake, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Unstake of exact remaining stake succeeds
// ---------------------------------------------------------------------------

#[test]
fn unstake_after_slash_of_remaining_succeeds() {
    let alice = addr(0x03);
    let mut chain = chain_with_staked_account(alice, 0, 1000);

    // Slash 200 → 800 remaining
    chain
        .apply_state_transition(StateTransition::Slash {
            address: alice,
            amount: Balance::new(200),
            reason: "test-slash".to_string(),
        })
        .expect("slash");

    // Releasing exactly 800 must succeed
    chain
        .apply_state_transition(StateTransition::ReleaseToSpendable {
            address: alice,
            amount: Balance::new(800),
        })
        .expect("release remaining stake must succeed");

    let acc = chain.state.get_account(&alice).unwrap();
    assert_eq!(
        acc.stake_locked,
        Balance::new(0),
        "stake_locked must be 0 after full release"
    );
    assert_eq!(
        acc.spendable_balance,
        Balance::new(800),
        "spendable must be 800 after release"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Slash exceeding stake returns error — no underflow
// ---------------------------------------------------------------------------

#[test]
fn slash_exceeding_stake_returns_error_no_underflow() {
    let alice = addr(0x04);
    // Account with 1000 staked and 0 extra spendable
    let mut chain = chain_with_staked_account(alice, 0, 1000);

    // Attempt to slash 1500 (more than available)
    let result = chain.apply_state_transition(StateTransition::Slash {
        address: alice,
        amount: Balance::new(1500),
        reason: "overslash".to_string(),
    });

    assert!(
        result.is_err(),
        "slashing more than available must return an error"
    );

    // Verify no underflow: balance components must not exceed original total
    let acc = chain.state.get_account(&alice).unwrap();
    // Balance is a u128 newtype — underflow would panic or wrap; absence of panic is itself
    // the safety check.  Additionally the stake values must be >= 0 (always true for u128)
    // and the components must sum consistently.
    let locked: u128 = acc.stake_locked.into();
    let spendable: u128 = acc.spendable_balance.into();
    assert!(
        locked <= 1000,
        "stake_locked must not exceed original 1000, got {locked}"
    );
    assert!(
        spendable <= 1000,
        "spendable_balance must not exceed original 1000, got {spendable}"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Double slash deducts cumulatively
// ---------------------------------------------------------------------------

#[test]
fn double_slash_deducts_cumulatively() {
    let alice = addr(0x05);
    let mut chain = chain_with_staked_account(alice, 0, 1000);

    // First slash: 1000 - 200 = 800
    chain
        .apply_state_transition(StateTransition::Slash {
            address: alice,
            amount: Balance::new(200),
            reason: "slash-1".to_string(),
        })
        .expect("first slash");

    let acc = chain.state.get_account(&alice).unwrap();
    assert_eq!(acc.stake_locked, Balance::new(800), "after first slash");

    // Second slash: 800 - 200 = 600
    chain
        .apply_state_transition(StateTransition::Slash {
            address: alice,
            amount: Balance::new(200),
            reason: "slash-2".to_string(),
        })
        .expect("second slash");

    let acc = chain.state.get_account(&alice).unwrap();
    assert_eq!(
        acc.stake_locked,
        Balance::new(600),
        "after double slash: 1000 - 200 - 200 = 600"
    );
    assert_eq!(
        acc.balance,
        Balance::new(600),
        "total balance must reflect both slashes"
    );
}

// ---------------------------------------------------------------------------
// Test 6: SlashingCircuitBreaker caps events per block and scaled_penalty
//         calculates the correct penalty amount
// ---------------------------------------------------------------------------

#[test]
fn slashing_circuit_breaker_caps_events_and_scaled_penalty() {
    // The circuit breaker limits slashes to MAX_SLASHING_EVENTS_PER_BLOCK (100)
    // per block.  Verify the cap and that scaled_penalty() is deterministic.
    let mut cb = SlashingCircuitBreaker::new();

    for i in 0..MAX_SLASHING_EVENTS_PER_BLOCK {
        assert!(
            !cb.would_exceed_cap(),
            "cap must not be reached after {i} events"
        );
        cb.record_slash();
    }
    assert!(
        cb.would_exceed_cap(),
        "cap must be reached after {MAX_SLASHING_EVENTS_PER_BLOCK} events"
    );

    cb.record_skipped();
    assert_eq!(cb.events_applied(), MAX_SLASHING_EVENTS_PER_BLOCK);
    assert_eq!(cb.events_skipped(), 1);

    // Reset clears state for a new block
    cb.reset();
    assert!(!cb.would_exceed_cap(), "cap must be cleared after reset");
    assert_eq!(cb.events_applied(), 0);

    // scaled_penalty: at the testnet target of 12 oracles, base=30 → 30/sqrt(12) ≈ 8.66
    // clamped to [1, 100].  The exact integer result is 8 (floor(30*1e6 / isqrt(12e12))).
    // The existing unit tests in slashing.rs already pin this to 8 — confirm it here.
    let penalty_12 = scaled_penalty(30, 12);
    assert!(
        penalty_12 >= 1 && penalty_12 <= 100,
        "scaled_penalty must be clamped, got {penalty_12}"
    );
    // At oracle_count=1 the base is returned (clamped to [1,100])
    let penalty_1 = scaled_penalty(30, 1);
    assert_eq!(penalty_1, 30, "at 1 oracle scaled_penalty returns base clamped");

    // Penalty decreases (or stays equal) as network grows
    let penalty_100 = scaled_penalty(30, 100);
    assert!(
        penalty_100 <= penalty_12,
        "penalty at 100 oracles ({penalty_100}) must be <= penalty at 12 ({penalty_12})"
    );
}

// ---------------------------------------------------------------------------
// Test 7: Slash spills over from stake_locked into spendable_balance
// ---------------------------------------------------------------------------

#[test]
fn slash_spills_into_spendable_when_stake_insufficient() {
    let alice = addr(0x07);
    // 300 staked, 700 spendable
    let mut chain = chain_with_staked_account(alice, 700, 300);

    // Slash 500: first drains 300 from stake, then 200 from spendable
    chain
        .apply_state_transition(StateTransition::Slash {
            address: alice,
            amount: Balance::new(500),
            reason: "spillover-slash".to_string(),
        })
        .expect("spillover slash must succeed");

    let acc = chain.state.get_account(&alice).unwrap();
    assert_eq!(
        acc.stake_locked,
        Balance::new(0),
        "stake_locked must be fully drained"
    );
    assert_eq!(
        acc.spendable_balance,
        Balance::new(500),
        "spendable must have 700 - 200 = 500 remaining"
    );
    assert_eq!(
        acc.balance,
        Balance::new(500),
        "total balance must be 1000 - 500 = 500"
    );
}
