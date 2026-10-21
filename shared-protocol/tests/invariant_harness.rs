//! Integration test: the invariant harness runs after every applied block.
//!
//! Story #2100, epic #2093.
//!
//! The unit tests in `invariants/harness.rs` check the harness against
//! hand-built states. This file checks it against states produced by the real
//! `State::apply_transition` path, which is the thing that has to stay
//! consistent. An invariant that only ever sees fixtures is an invariant that
//! has never been tested against the system it guards.
//!
//! # Why not `tests/integration/`
//!
//! The story's plan says to wire this into `tests/integration/`. That crate
//! (`test_server`) declares only `tokio`, `serde` and `serde_json` as
//! dependencies and carries its own toy `Blockchain` with a `String`-hashed
//! block — it does not depend on `wrfcoin-shared-protocol` and has no
//! `State`, no `StateTransition` and no block-apply path to hook. There is
//! nothing there to wire the harness into.
//!
//! The real block-apply integration tests live in `shared-protocol/tests/`,
//! alongside `consensus_invariants.rs` which already gates on
//! `invariants::all_invariants`. This file joins them.

use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::invariants::harness::{Invariant, InvariantHarness};

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

/// The sender's next acceptable transfer nonce.
///
/// `check_and_reserve_nonce` requires `tx_nonce == account.nonce` exactly, and
/// nonce enforcement is active from genesis height 0, so a transfer built with
/// `nonce: None` is rejected outright. Reading it from state keeps the test
/// honest about using the real apply path rather than a bypass.
fn next_nonce(state: &State, sender: &Address) -> u64 {
    state.accounts.get(sender).map(|a| a.nonce).unwrap_or(0)
}

/// A state with two funded accounts whose parts sum correctly.
fn funded_state() -> State {
    let mut state = State::new();
    for byte in [1u8, 2] {
        let acct = state.create_account(addr(byte), Balance::new(10_000));
        acct.spendable_balance = Balance::new(10_000);
        acct.stake_locked = Balance::new(0);
    }
    state
}

/// The core requirement: apply a sequence of real transitions and evaluate
/// every registered invariant after each one. A violation anywhere in the
/// sequence fails the test, naming every invariant that tripped.
#[test]
fn harness_runs_after_every_applied_transition() {
    let harness = InvariantHarness::with_defaults();
    let mut state = funded_state();

    assert_eq!(
        harness.check_all(&state),
        Ok(()),
        "precondition: the starting state must be clean"
    );

    let transitions = vec![
        StateTransition::LockToStake {
            address: addr(1),
            amount: Balance::new(2_500),
        },
        StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(2),
            amount: Balance::new(1_000),
            nonce: Some(next_nonce(&state, &addr(1))),
            current_block: 1,
        },
        StateTransition::ReleaseToSpendable {
            address: addr(1),
            amount: Balance::new(500),
        },
        StateTransition::ApplyReward {
            address: addr(2),
            amount: Balance::new(250),
            reason: "invariant-harness-integration".to_string(),
        },
    ];

    for (i, transition) in transitions.into_iter().enumerate() {
        let pre = state.clone();

        if let Err(e) = state.apply_transition(transition.clone()) {
            panic!("transition {i} ({transition:?}) failed to apply: {e}");
        }

        // Transition invariants: pre -> post.
        if let Err(violations) = harness.check_transition(&pre, &state, &transition) {
            panic!(
                "transition {i} ({transition:?}) violated {} invariant(s): {}",
                violations.len(),
                render(&violations),
            );
        }

        // State invariants: the resulting state on its own.
        if let Err(violations) = harness.check_all(&state) {
            panic!(
                "state after transition {i} violated {} invariant(s): {}",
                violations.len(),
                render(&violations),
            );
        }
    }
}

/// A deliberately corrupted state fails with the expected invariant ID, and
/// reports *every* violated invariant rather than stopping at the first.
#[test]
fn corrupted_state_fixture_reports_all_expected_ids() {
    let harness = InvariantHarness::with_defaults();
    let mut state = funded_state();

    // Break two unrelated invariants at once.
    state
        .accounts
        .get_mut(&addr(1))
        .expect("fixture account")
        .stake_locked = Balance::new(7);
    state
        .governance
        .params
        .pop_liveness_policy
        .useful_work_activation_height = 99;

    let violations = harness
        .check_all(&state)
        .expect_err("a corrupted state must not pass");

    let ids: Vec<&str> = violations.iter().map(|(id, _)| *id).collect();
    assert!(
        ids.iter().any(|id| id.contains("INV-BAL-2")),
        "expected the balance accounting identity to trip; got {ids:?}"
    );
    assert!(
        ids.iter().any(|id| id.contains("INV-POP-1")),
        "expected the useful-work lock to trip; got {ids:?}"
    );
    assert_eq!(
        violations.len(),
        2,
        "aggregation must report both, not the first: {}",
        render(&violations)
    );
}

/// The extension path #2140, #2190, #2167 and #2204 will take, exercised
/// end-to-end against real applied state rather than a fixture.
#[test]
fn a_downstream_epic_can_register_and_be_enforced() {
    /// Stand-in for an invariant a later epic registers.
    struct NoZeroBalanceAccounts;

    impl Invariant for NoZeroBalanceAccounts {
        fn id(&self) -> &'static str {
            "INV-EXAMPLE-2: No Zero-Balance Accounts"
        }

        fn check(&self, state: &State) -> Result<(), String> {
            for (addr, acct) in &state.accounts {
                if acct.balance == Balance::new(0) {
                    return Err(format!("account {addr} has a zero balance"));
                }
            }
            Ok(())
        }
    }

    let harness = InvariantHarness::with_defaults().with(Box::new(NoZeroBalanceAccounts));
    harness
        .assert_unique_ids()
        .expect("a registered invariant must not collide with a shipped id");

    let mut state = funded_state();
    assert_eq!(harness.check_all(&state), Ok(()));

    // Create an account with nothing in it and confirm the registered
    // invariant — not just the built-in ones — is enforced.
    state.create_account(addr(9), Balance::new(0));

    let violations = harness
        .check_all(&state)
        .expect_err("the registered invariant must be enforced");
    assert!(
        violations
            .iter()
            .any(|(id, _)| id.contains("INV-EXAMPLE-2")),
        "got {}",
        render(&violations)
    );
}

/// Long randomised-ish sequence: the harness must stay green across many
/// applied transitions, not just a handful. Deterministic by construction —
/// no RNG, no clock — so a failure is always reproducible.
#[test]
fn harness_stays_green_across_a_long_transition_sequence() {
    let harness = InvariantHarness::with_defaults();
    let mut state = funded_state();

    for round in 0..50u64 {
        let transition = match round % 3 {
            0 => StateTransition::LockToStake {
                address: addr(1),
                amount: Balance::new(10),
            },
            1 => StateTransition::BalanceTransfer {
                from: addr(2),
                to: addr(1),
                amount: Balance::new(5),
                nonce: Some(next_nonce(&state, &addr(2))),
                current_block: round + 1,
            },
            _ => StateTransition::ReleaseToSpendable {
                address: addr(1),
                amount: Balance::new(10),
            },
        };

        let pre = state.clone();
        state
            .apply_transition(transition.clone())
            .unwrap_or_else(|e| panic!("round {round}: {transition:?} failed to apply: {e}"));

        if let Err(violations) = harness.check_transition(&pre, &state, &transition) {
            panic!(
                "round {round}: transition invariants: {}",
                render(&violations)
            );
        }
        if let Err(violations) = harness.check_all(&state) {
            panic!("round {round}: state invariants: {}", render(&violations));
        }
    }
}

fn render(violations: &[(&'static str, String)]) -> String {
    violations
        .iter()
        .map(|(id, detail)| format!("[{id}] {detail}"))
        .collect::<Vec<_>>()
        .join("; ")
}
