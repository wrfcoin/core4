//! Reward-related consensus invariants.

use super::harness::InvariantHarness;
use super::{ConsensusInvariant, InvariantResult};
use crate::blockchain::state::{State, StateTransition};
use crate::common::types::Balance;
use crate::consensus::proof_of_publishing::MAX_BLOCK_EMISSION;

/// Default per-transition emission cap aligned with the consensus block emission ceiling.
pub const DEFAULT_EMISSION_CAP: u128 = MAX_BLOCK_EMISSION;

pub struct RewardMonotonicity;

impl ConsensusInvariant for RewardMonotonicity {
    fn name(&self) -> &'static str {
        "INV-RWD-1: Reward Monotonicity"
    }

    fn check(
        &self,
        pre: &State,
        post: &State,
        _transition: &StateTransition,
    ) -> Option<InvariantResult> {
        for (addr, post_acct) in &post.accounts {
            let pre_earned = pre
                .accounts
                .get(addr)
                .map(|a| a.earned_balance_total)
                .unwrap_or(Balance::new(0));
            if post_acct.earned_balance_total < pre_earned {
                return Some(InvariantResult::fail(
                    self.name(),
                    format!(
                        "Account {}: earned_balance_total decreased from {} to {}",
                        addr, pre_earned, post_acct.earned_balance_total,
                    ),
                ));
            }
        }
        Some(InvariantResult::pass(self.name()))
    }
}

pub struct EmissionCap {
    pub cap: u128,
}

impl EmissionCap {
    pub fn new(cap: u128) -> Self {
        Self { cap }
    }
}

impl Default for EmissionCap {
    fn default() -> Self {
        Self {
            cap: DEFAULT_EMISSION_CAP,
        }
    }
}

impl ConsensusInvariant for EmissionCap {
    fn name(&self) -> &'static str {
        "INV-RWD-2: Emission Cap"
    }

    fn check(
        &self,
        _pre: &State,
        _post: &State,
        transition: &StateTransition,
    ) -> Option<InvariantResult> {
        match transition {
            StateTransition::ApplyReward { amount, .. } => {
                if *amount <= Balance::new(self.cap) {
                    Some(InvariantResult::pass(self.name()))
                } else {
                    Some(InvariantResult::fail(
                        self.name(),
                        format!("Reward amount {} exceeds emission cap {}", amount, self.cap,),
                    ))
                }
            },
            _ => None,
        }
    }
}

/// Register every reward invariant with the harness (#2260). Owned by this
/// module; see [`super::balance::register_invariants`] for the rationale.
pub fn register_invariants(harness: &mut InvariantHarness) {
    harness.register_transition(Box::new(RewardMonotonicity));
    harness.register_transition(Box::new(EmissionCap::default()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::state::State;
    use crate::common::types::Address;

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    #[test]
    fn earned_total_increases_on_reward() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(0));

        let reward = StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(50),
            reason: "test".to_string(),
        };

        let mut post = pre.clone();
        post.apply_transition(reward.clone()).unwrap();

        let result = RewardMonotonicity.check(&pre, &post, &reward).unwrap();
        assert!(result.passed, "Expected pass: {}", result.detail);
    }

    #[test]
    fn earned_total_decrease_detected() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(0));
        pre.apply_transition(StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(100),
            reason: "seed".to_string(),
        })
        .unwrap();

        // Create post state by cloning and manually corrupting earned_balance_total.
        // Use a LockToStake transition as a benign placeholder -- the invariant
        // fires on every transition, not just rewards.
        let mut post = pre.clone();
        let transition = StateTransition::LockToStake {
            address: addr(1),
            amount: Balance::new(0),
        };
        post.accounts
            .get_mut(&addr(1))
            .unwrap()
            .earned_balance_total = Balance::new(50);

        let result = RewardMonotonicity.check(&pre, &post, &transition).unwrap();
        assert!(!result.passed, "Expected failure");
        assert!(result.detail.contains("decreased"));
    }

    #[test]
    fn emission_cap_pass_under_limit() {
        let pre = State::new();
        let post = State::new();

        let reward = StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(100),
            reason: "test".to_string(),
        };

        let result = EmissionCap::default().check(&pre, &post, &reward).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn emission_cap_fail_over_limit() {
        let pre = State::new();
        let post = State::new();

        let reward = StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(MAX_BLOCK_EMISSION + 1),
            reason: "test".to_string(),
        };

        let result = EmissionCap::default().check(&pre, &post, &reward).unwrap();
        assert!(!result.passed);
        assert!(result.detail.contains("exceeds"));
    }

    #[test]
    fn expected_fail_reward_decreases_earned_total() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(0));
        pre.apply_transition(StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(200),
            reason: "seed".to_string(),
        })
        .unwrap();

        let reward = StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(50),
            reason: "test".to_string(),
        };

        let mut post = pre.clone();
        post.apply_transition(reward.clone()).unwrap();
        post.accounts
            .get_mut(&addr(1))
            .unwrap()
            .earned_balance_total = Balance::new(100);

        let result = RewardMonotonicity.check(&pre, &post, &reward).unwrap();
        assert!(
            !result.passed,
            "Invariant should catch earned_total decrease"
        );
    }

    #[test]
    fn emission_cap_configurable() {
        let pre = State::new();
        let post = State::new();

        let reward = StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(500),
            reason: "test".to_string(),
        };

        // With cap=200, amount=500 should fail
        let strict = EmissionCap::new(200);
        let result = strict.check(&pre, &post, &reward).unwrap();
        assert!(!result.passed, "500 should exceed cap of 200");

        // With cap=1000, amount=500 should pass
        let lenient = EmissionCap::new(1_000);
        let result = lenient.check(&pre, &post, &reward).unwrap();
        assert!(result.passed, "500 should be within cap of 1000");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::blockchain::state::State;
    use crate::common::types::Address;
    use proptest::prelude::*;

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    proptest! {
        #[test]
        fn emission_cap_consistent(amount in 0u128..10_000, cap in 1u128..10_000) {
            let invariant = EmissionCap::new(cap);
            let pre = State::new();
            let post = State::new();
            let reward = StateTransition::ApplyReward {
                address: addr(1),
                amount: Balance::new(amount),
                reason: "proptest".to_string(),
            };
            let result = invariant.check(&pre, &post, &reward).unwrap();
            prop_assert_eq!(result.passed, amount <= cap);
        }

        #[test]
        fn reward_monotonicity_holds_for_valid_rewards(amount in 1u128..10_000) {
            let mut pre = State::new();
            pre.create_account(addr(1), Balance::new(0));

            let reward = StateTransition::ApplyReward {
                address: addr(1),
                amount: Balance::new(amount),
                reason: "proptest".to_string(),
            };

            let mut post = pre.clone();
            post.apply_transition(reward.clone()).unwrap();

            let result = RewardMonotonicity.check(&pre, &post, &reward).unwrap();
            prop_assert!(result.passed, "Valid reward should never violate monotonicity");
        }
    }
}
