//! Bounded model checker for consensus state invariants.
//!
//! Performs exhaustive BFS over a reduced state space, applying every
//! valid `StateTransition` at each step and checking all registered
//! invariants after every transition.

use super::{ConsensusInvariant, check_all_invariants};
use crate::blockchain::state::{State, StateTransition};
use crate::common::types::{Address, Balance};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Configuration for a model-checking run.
#[derive(Debug, Clone)]
pub struct ModelCheckConfig {
    pub num_accounts: usize,
    pub initial_balances: Vec<u128>,
    pub amounts: Vec<u128>,
    pub max_depth: usize,
    pub stake_required: Vec<u128>,
}

impl Default for ModelCheckConfig {
    fn default() -> Self {
        Self {
            num_accounts: 2,
            initial_balances: vec![100, 50],
            amounts: vec![0, 1, 50, 100],
            max_depth: 4,
            stake_required: vec![0, 0],
        }
    }
}

/// Summary of a model-checking run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCheckReport {
    pub states_explored: u64,
    pub transitions_applied: u64,
    pub invariant_checks: u64,
    pub violations: Vec<Violation>,
    pub max_depth_reached: usize,
    pub all_passed: bool,
}

/// A single invariant violation with trace context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    pub invariant: String,
    pub detail: String,
    pub depth: usize,
    pub transition: String,
}

fn model_address(index: usize) -> Address {
    let byte = (index + 1) as u8;
    Address([byte; 20])
}

#[allow(clippy::excessive_nesting)]
fn generate_transitions(config: &ModelCheckConfig, state: &State) -> Vec<StateTransition> {
    let mut transitions = Vec::new();
    let n = config.num_accounts;

    for &amount in &config.amounts {
        if amount == 0 {
            continue;
        }

        for from_idx in 0..n {
            let from = model_address(from_idx);

            // BalanceTransfer
            for to_idx in 0..n {
                if from_idx == to_idx {
                    continue;
                }
                let to = model_address(to_idx);
                if let Some(acct) = state.accounts.get(&from) {
                    let amount = Balance::new(amount);
                    if acct.spendable_balance >= amount && acct.balance >= amount {
                        transitions.push(StateTransition::BalanceTransfer {
                            from,
                            to,
                            amount,
                            nonce: Some(acct.nonce),
                            current_block: 0,
                        });
                    }
                }
            }

            // LockToStake
            if let Some(acct) = state.accounts.get(&from) {
                let amount = Balance::new(amount);
                if acct.spendable_balance >= amount {
                    transitions.push(StateTransition::LockToStake {
                        address: from,
                        amount,
                    });
                }
            }

            // ReleaseToSpendable
            if let Some(acct) = state.accounts.get(&from) {
                let amount = Balance::new(amount);
                if acct.stake_locked >= amount {
                    transitions.push(StateTransition::ReleaseToSpendable {
                        address: from,
                        amount,
                    });
                }
            }

            // ApplyReward (capped at 100 for model checking)
            if amount <= 100 {
                transitions.push(StateTransition::ApplyReward {
                    address: from,
                    amount: Balance::new(amount),
                    reason: "model_check".to_string(),
                });
            }

            // Slash
            if let Some(acct) = state.accounts.get(&from) {
                let amount = Balance::new(amount);
                if acct.balance >= amount {
                    transitions.push(StateTransition::Slash {
                        address: from,
                        amount,
                        reason: "model_check".to_string(),
                    });
                }
            }
        }
    }

    transitions
}

/// Run the bounded model checker.
pub fn run(
    config: &ModelCheckConfig,
    invariants: &[Box<dyn ConsensusInvariant>],
) -> ModelCheckReport {
    let mut report = ModelCheckReport {
        states_explored: 0,
        transitions_applied: 0,
        invariant_checks: 0,
        violations: Vec::new(),
        max_depth_reached: 0,
        all_passed: true,
    };

    // Build initial state.
    let mut initial = State::new();
    for i in 0..config.num_accounts {
        let addr = model_address(i);
        let balance = config.initial_balances.get(i).copied().unwrap_or(0);
        initial.create_account(addr, Balance::new(balance));
        let stake_req = config.stake_required.get(i).copied().unwrap_or(0);
        if stake_req > 0 {
            initial.set_stake_required(addr, Balance::new(stake_req));
        }
    }

    // BFS queue: (state, depth)
    let mut queue: VecDeque<(State, usize)> = VecDeque::new();
    queue.push_back((initial, 0));

    while let Some((state, depth)) = queue.pop_front() {
        report.states_explored += 1;
        if depth > report.max_depth_reached {
            report.max_depth_reached = depth;
        }
        if depth >= config.max_depth {
            continue;
        }

        let transitions = generate_transitions(config, &state);

        for transition in transitions {
            let pre = state.clone();
            let mut post = state.clone();

            if post.apply_transition(transition.clone()).is_err() {
                continue;
            }

            report.transitions_applied += 1;

            let results = check_all_invariants(invariants, &pre, &post, &transition);
            for result in &results {
                report.invariant_checks += 1;
                if !result.passed {
                    report.all_passed = false;
                    report.violations.push(Violation {
                        invariant: result.invariant_name.to_string(),
                        detail: result.detail.clone(),
                        depth,
                        transition: format!("{transition:?}"),
                    });
                }
            }

            // Only enqueue if no violations (fail-fast) and within depth.
            if report.all_passed && depth + 1 < config.max_depth {
                queue.push_back((post, depth + 1));
            }
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invariants::all_invariants;

    #[test]
    fn default_model_check_passes() {
        let config = ModelCheckConfig::default();
        let invariants = all_invariants();
        let report = run(&config, &invariants);

        assert!(
            report.all_passed,
            "Model check failed with {} violations: {:?}",
            report.violations.len(),
            report.violations,
        );
        assert!(report.states_explored > 0);
        assert!(report.transitions_applied > 0);
        assert!(report.invariant_checks > 0);
    }

    #[test]
    fn model_check_with_stake_requirements() {
        let config = ModelCheckConfig {
            num_accounts: 2,
            initial_balances: vec![100, 0],
            amounts: vec![1, 50],
            max_depth: 4,
            stake_required: vec![30, 20],
        };
        let invariants = all_invariants();
        let report = run(&config, &invariants);

        assert!(
            report.all_passed,
            "Model check failed: {:?}",
            report.violations,
        );
    }

    #[test]
    fn model_check_is_deterministic() {
        let config = ModelCheckConfig {
            num_accounts: 2,
            initial_balances: vec![50, 50],
            amounts: vec![1, 25, 50],
            max_depth: 3,
            stake_required: vec![0, 0],
        };
        let invariants = all_invariants();

        let report_a = run(&config, &invariants);
        let report_b = run(&config, &invariants);

        assert_eq!(report_a.states_explored, report_b.states_explored);
        assert_eq!(report_a.transitions_applied, report_b.transitions_applied);
        assert_eq!(report_a.invariant_checks, report_b.invariant_checks);
    }
}
