//! Slashing invariant.

use super::harness::InvariantHarness;
use super::{ConsensusInvariant, InvariantResult};
use crate::blockchain::state::{State, StateTransition};

pub struct SlashingBound;

impl ConsensusInvariant for SlashingBound {
    fn name(&self) -> &'static str {
        "INV-SLASH-1: Slashing Bound"
    }

    fn check(
        &self,
        pre: &State,
        post: &State,
        transition: &StateTransition,
    ) -> Option<InvariantResult> {
        let address = match transition {
            StateTransition::Slash { address, .. } => address,
            _ => return None,
        };

        let pre_acct = match pre.accounts.get(address) {
            Some(a) => a,
            None => {
                return Some(InvariantResult::fail(
                    self.name(),
                    format!("Pre-state account {address} not found"),
                ));
            },
        };

        let post_acct = match post.accounts.get(address) {
            Some(a) => a,
            None => {
                return Some(InvariantResult::fail(
                    self.name(),
                    format!("Post-state account {address} not found"),
                ));
            },
        };

        let mut violations = Vec::new();

        if post_acct.balance > pre_acct.balance {
            violations.push(format!(
                "balance increased: {} -> {}",
                pre_acct.balance, post_acct.balance
            ));
        }
        if post_acct.stake_locked > pre_acct.stake_locked {
            violations.push(format!(
                "stake_locked increased: {} -> {}",
                pre_acct.stake_locked, post_acct.stake_locked
            ));
        }
        if post_acct.spendable_balance > pre_acct.spendable_balance {
            violations.push(format!(
                "spendable_balance increased: {} -> {}",
                pre_acct.spendable_balance, post_acct.spendable_balance
            ));
        }

        if violations.is_empty() {
            Some(InvariantResult::pass(self.name()))
        } else {
            Some(InvariantResult::fail(
                self.name(),
                format!("Account {}: {}", address, violations.join("; ")),
            ))
        }
    }
}

/// Register every slashing invariant with the harness (#2260). Owned by this
/// module; see [`super::balance::register_invariants`] for the rationale.
pub fn register_invariants(harness: &mut InvariantHarness) {
    harness.register_transition(Box::new(SlashingBound));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::state::State;
    use crate::common::types::{Address, Balance};

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    #[test]
    fn slash_reduces_balance() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(100));
        pre.apply_transition(StateTransition::LockToStake {
            address: addr(1),
            amount: Balance::new(40),
        })
        .unwrap();

        let slash = StateTransition::Slash {
            address: addr(1),
            amount: Balance::new(50),
            reason: "test".to_string(),
        };

        let mut post = pre.clone();
        post.apply_transition(slash.clone()).unwrap();

        let result = SlashingBound.check(&pre, &post, &slash).unwrap();
        assert!(result.passed, "Expected pass: {}", result.detail);
    }

    #[test]
    fn slash_detects_increase() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(100));

        let slash = StateTransition::Slash {
            address: addr(1),
            amount: Balance::new(10),
            reason: "test".to_string(),
        };

        let mut post = pre.clone();
        post.apply_transition(slash.clone()).unwrap();
        post.accounts.get_mut(&addr(1)).unwrap().balance += Balance::new(50);

        let result = SlashingBound.check(&pre, &post, &slash).unwrap();
        assert!(!result.passed, "Expected failure");
        assert!(result.detail.contains("balance increased"));
    }

    #[test]
    fn expected_fail_slash_increases_spendable() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(100));
        pre.apply_transition(StateTransition::LockToStake {
            address: addr(1),
            amount: Balance::new(40),
        })
        .unwrap();

        let slash = StateTransition::Slash {
            address: addr(1),
            amount: Balance::new(20),
            reason: "test".to_string(),
        };

        let mut post = pre.clone();
        post.apply_transition(slash.clone()).unwrap();
        let post_acct = post.accounts.get_mut(&addr(1)).unwrap();
        post_acct.spendable_balance = pre.accounts[&addr(1)].spendable_balance + Balance::new(10);

        let result = SlashingBound.check(&pre, &post, &slash).unwrap();
        assert!(
            !result.passed,
            "Invariant should catch spendable increase after slash"
        );
    }

    #[test]
    fn slash_missing_pre_account_returns_fail_not_panic() {
        // Pre-state does not contain the slashed address.
        // SlashingBound must return InvariantResult::fail (not panic).
        let pre = State::new();
        let mut post = State::new();
        post.create_account(addr(1), Balance::new(100));

        let slash = StateTransition::Slash {
            address: addr(1),
            amount: Balance::new(10),
            reason: "test".to_string(),
        };

        let result = SlashingBound.check(&pre, &post, &slash);
        assert!(result.is_some(), "invariant should return Some for Slash transition");
        let r = result.unwrap();
        assert!(!r.passed, "missing pre-state account must be reported as failure");
        assert!(
            r.detail.contains("not found"),
            "detail should mention the missing account; got: {}",
            r.detail
        );
    }
}
