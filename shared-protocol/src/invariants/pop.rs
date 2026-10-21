//! PoP useful-work consensus invariants.

use super::harness::{Invariant, InvariantHarness};
use super::{ConsensusInvariant, InvariantResult};
use crate::blockchain::state::{State, StateTransition};

/// Enforces useful-work-only mode by requiring the activation height lock to be zero.
pub struct UsefulWorkActivationNonRegression;

impl ConsensusInvariant for UsefulWorkActivationNonRegression {
    fn name(&self) -> &'static str {
        "INV-POP-1: Useful-Work Activation Non-Regression"
    }

    fn check(
        &self,
        pre: &State,
        post: &State,
        _transition: &StateTransition,
    ) -> Option<InvariantResult> {
        let pre_height = pre
            .governance
            .params
            .pop_liveness_policy
            .useful_work_activation_height;
        let post_height = post
            .governance
            .params
            .pop_liveness_policy
            .useful_work_activation_height;

        if post_height != 0 {
            return Some(InvariantResult::fail(
                self.name(),
                format!(
                    "useful-work-only mode requires pop.useful_work_activation_height=0, got {}",
                    post_height
                ),
            ));
        }

        if pre_height == 0 {
            return None;
        }

        // Permit transitions that explicitly normalize a legacy/non-zero pre-state
        // into the useful-work-only lock state.
        Some(InvariantResult::pass(self.name()))
    }
}

// ---------------------------------------------------------------------------
// State form of INV-POP-1, and this domain's harness registration.
// ---------------------------------------------------------------------------

/// INV-POP-1 as a state predicate: useful-work-only mode requires the
/// activation-height lock to be zero.
///
/// Shares an id with [`UsefulWorkActivationNonRegression`] deliberately — it
/// is the same invariant, and the zero-lock condition is the whole of it in
/// useful-work-only mode. [`register_invariants`] registers only this form
/// because it subsumes the transition form.
pub struct UsefulWorkActivationLockState;

impl Invariant for UsefulWorkActivationLockState {
    fn id(&self) -> &'static str {
        "INV-POP-1: Useful-Work Activation Non-Regression"
    }

    fn check(&self, state: &State) -> Result<(), String> {
        let height = state
            .governance
            .params
            .pop_liveness_policy
            .useful_work_activation_height;
        if height != 0 {
            return Err(format!(
                "useful-work-only mode requires pop.useful_work_activation_height=0, got {height}"
            ));
        }
        Ok(())
    }
}

/// Register every PoP invariant with the harness (#2260). Owned by this
/// module; see [`super::balance::register_invariants`] for the rationale.
pub fn register_invariants(harness: &mut InvariantHarness) {
    harness.register(Box::new(UsefulWorkActivationLockState));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_transition_into_useful_work_lock_state() {
        let mut pre = State::new();
        pre.governance.params.pop_liveness_policy.useful_work_activation_height = 500;
        let mut post = pre.clone();
        post.governance.params.pop_liveness_policy.useful_work_activation_height = 0;

        let transition = StateTransition::LockToStake {
            address: crate::common::types::Address([1; 20]),
            amount: crate::common::types::Balance::new(0),
        };
        let result = UsefulWorkActivationNonRegression
            .check(&pre, &post, &transition)
            .expect("activation height changed so invariant should report");
        assert!(result.passed, "expected pass, got {}", result.detail);
    }

    #[test]
    fn rejects_non_zero_post_state_activation_height() {
        let mut pre = State::new();
        pre.governance.params.pop_liveness_policy.useful_work_activation_height = 0;
        let mut post = pre.clone();
        post.governance
            .params
            .pop_liveness_policy
            .useful_work_activation_height = 1;

        let transition = StateTransition::LockToStake {
            address: crate::common::types::Address([2; 20]),
            amount: crate::common::types::Balance::new(0),
        };
        let result = UsefulWorkActivationNonRegression
            .check(&pre, &post, &transition)
            .expect("activation height changed so invariant should report");
        assert!(!result.passed, "expected failure");
        assert!(result.detail.contains("requires"));
    }
}
