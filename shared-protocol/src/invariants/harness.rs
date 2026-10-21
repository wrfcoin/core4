//! One entry point that evaluates every registered invariant (#2100).
//!
//! Invariants that must be called by hand get called by nobody. This module
//! makes "check everything after every applied block" the default rather than
//! an aspiration, and makes registering a new invariant a one-line change.
//!
//! # Two kinds of invariant, one harness
//!
//! [`Invariant`] is a predicate over a **single** [`State`]: "is this state
//! internally consistent?". It is what you can run after a block has been
//! applied, when the pre-state is gone.
//!
//! [`ConsensusInvariant`] — the pre-existing trait in
//! [`super`] — is a predicate over a **transition**: `(pre, post, transition)`.
//! Conservation properties need it, because "total balance did not change"
//! is not a statement about any one state.
//!
//! Both are registered here. [`InvariantHarness::check_all`] runs the state
//! invariants; [`InvariantHarness::check_transition`] runs the transition
//! invariants. Neither kind was collapsed into the other: forcing a
//! conservation invariant through a state-only interface would make it check
//! nothing while still reporting a pass, which is the failure mode this
//! module exists to prevent.
//!
//! # Extending this — for #2140, #2190, #2167 and #2204
//!
//! Each domain module owns its own registration hook, listed in
//! [`DOMAIN_REGISTRATIONS`]. Adding an invariant is:
//!
//! 1. Write it in your own module under `invariants/`, implementing
//!    [`Invariant`] (state predicate) or [`ConsensusInvariant`] (transition
//!    predicate). Do not edit anyone else's module.
//! 2. Register it from **your own module's** `register_invariants` hook — see
//!    [`super::balance::register_invariants`]. Or, for a one-off that should
//!    not run everywhere, at the call site:
//!
//! ```ignore
//! let harness = InvariantHarness::with_defaults()
//!     .with(Box::new(my_epic::MyInvariant::default()));
//! ```
//!
//! ## What that does and does not buy — the conflict claim, corrected
//!
//! An earlier revision of this module claimed a conflict-free extension
//! surface. **That claim was wrong and is withdrawn.** What is true:
//!
//! - Extending an **existing** domain — which is what #2250 (#2097, supply
//!   conservation in [`super::balance`]) actually does — touches only that
//!   domain's file. Two epics extending two different domains cannot conflict.
//!   This is the case the earlier revision got right by accident.
//! - Adding a **new** domain module still appends one line to
//!   [`DOMAIN_REGISTRATIONS`] and one `pub mod` line in [`super`]. Two epics
//!   inserting at the same alphabetical position produce an ordinary Git
//!   conflict. There is no way around that in safe Rust without a
//!   link-section registry crate (`linkme`, `inventory`), which
//!   [`super::registration_audit`] documents and explains why it was not
//!   adopted here.
//! - The `.with(...)` builder does **not** make a registration permanent: the
//!   shipped runner constructs [`InvariantHarness::with_defaults`] only. Use
//!   it for call-site experiments, not for anything that must always run.
//!
//! So: one one-line conflict per *new domain*, none per *new invariant*. The
//! guarantee that an omission is caught comes from
//! [`super::registration_audit`], not from the absence of conflicts.
//!
//! Two rules that keep this workable for four concurrent epics:
//!
//! - **IDs are namespaced and unique.** Use `INV-<DOMAIN>-<n>`.
//!   [`InvariantHarness::assert_unique_ids`] enforces it, and there is a test
//!   that runs it against the defaults. Two epics shipping `INV-1` would make
//!   failure reports ambiguous. The same convention is what
//!   [`super::registration_audit`] scans for, so an identifier that does not
//!   follow it is invisible to the completeness audit.
//! - **`check` never panics.** It returns `Err(String)` describing the
//!   violation. A panicking invariant takes down the harness and hides every
//!   other violation in the same run.
//!
//! # Registered is not the same as complete
//!
//! This harness guarantees that everything registered runs. It cannot notice
//! that something was never registered — a registry that fails to notice an
//! omission reports a healthy system while a predicate runs nowhere. That gap
//! is closed by [`super::registration_audit`], which derives the expected set
//! from the source tree instead of trusting this list.

use super::{ConsensusInvariant, balance, check_all_invariants, pop, rewards, slashing};
use crate::blockchain::state::{State, StateTransition};

/// A domain module's registration hook: it receives the harness and registers
/// that domain's invariants, of either kind.
pub type DomainRegistration = fn(&mut InvariantHarness);

/// Every domain that registers invariants, paired with its owned hook.
///
/// The hook bodies live in the domain modules, so extending an existing
/// domain touches only that module. Adding a *new* domain still appends one
/// line here and one `pub mod` line in [`super`] — see the module docs on
/// what remains conflict-prone and what does not.
pub const DOMAIN_REGISTRATIONS: &[(&str, DomainRegistration)] = &[
    ("balance", balance::register_invariants),
    ("pop", pop::register_invariants),
    ("rewards", rewards::register_invariants),
    ("slashing", slashing::register_invariants),
];

// The state forms of INV-BAL-2 and INV-POP-1 moved to the domain modules that
// own them (#2260). Re-exported so existing paths keep working.
pub use super::balance::BalanceAccountingIdentityState;
pub use super::pop::UsefulWorkActivationLockState;

/// A predicate over a single state.
///
/// Implementors must be cheap enough to run after every applied block, and
/// must not panic — return `Err` instead.
pub trait Invariant: Send + Sync {
    /// Stable, unique identifier, e.g. `"INV-BAL-2: Balance Accounting Identity"`.
    ///
    /// Used in failure reports and in [`InvariantHarness::assert_unique_ids`].
    fn id(&self) -> &'static str;

    /// `Ok(())` if the state satisfies this invariant, otherwise a
    /// description of the violation specific enough to act on — name the
    /// account, height or field, not just the rule.
    fn check(&self, state: &State) -> Result<(), String>;
}

/// A violated invariant: its id and what went wrong.
pub type Violation = (&'static str, String);

/// The registry.
#[derive(Default)]
pub struct InvariantHarness {
    state_invariants: Vec<Box<dyn Invariant>>,
    transition_invariants: Vec<Box<dyn ConsensusInvariant>>,
}

impl InvariantHarness {
    /// An empty harness. Prefer [`with_defaults`](Self::with_defaults) unless
    /// you are testing a single invariant in isolation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every invariant that should run everywhere.
    ///
    /// Delegates to each domain's own [`DOMAIN_REGISTRATIONS`] hook rather
    /// than listing invariants here, so extending a domain never touches this
    /// function. See the module docs for what that does and does not buy.
    pub fn with_defaults() -> Self {
        let mut harness = Self::new();
        for (_domain, register) in DOMAIN_REGISTRATIONS {
            register(&mut harness);
        }
        harness
    }

    /// Register a state invariant.
    pub fn register(&mut self, invariant: Box<dyn Invariant>) -> &mut Self {
        self.state_invariants.push(invariant);
        self
    }

    /// Register a transition invariant.
    pub fn register_transition(&mut self, invariant: Box<dyn ConsensusInvariant>) -> &mut Self {
        self.transition_invariants.push(invariant);
        self
    }

    /// Builder form of [`register`](Self::register), for call-site extension.
    pub fn with(mut self, invariant: Box<dyn Invariant>) -> Self {
        self.state_invariants.push(invariant);
        self
    }

    /// Builder form of [`register_transition`](Self::register_transition).
    pub fn with_transition(mut self, invariant: Box<dyn ConsensusInvariant>) -> Self {
        self.transition_invariants.push(invariant);
        self
    }

    /// Evaluate every registered **state** invariant.
    ///
    /// Reports **all** violations, not the first. When a state is corrupt it
    /// is usually corrupt in several ways at once, and the first failure is
    /// rarely the most informative one — stopping early turns one debugging
    /// session into several.
    pub fn check_all(&self, state: &State) -> Result<(), Vec<Violation>> {
        let violations: Vec<Violation> = self
            .state_invariants
            .iter()
            .filter_map(|inv| inv.check(state).err().map(|detail| (inv.id(), detail)))
            .collect();

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }

    /// Evaluate every registered **transition** invariant around one applied
    /// transition. Also aggregates rather than short-circuiting.
    pub fn check_transition(
        &self,
        pre: &State,
        post: &State,
        transition: &StateTransition,
    ) -> Result<(), Vec<Violation>> {
        let violations: Vec<Violation> =
            check_all_invariants(&self.transition_invariants, pre, post, transition)
                .into_iter()
                .filter(|r| !r.passed)
                .map(|r| (r.invariant_name, r.detail))
                .collect();

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }

    /// Ids of every registered invariant, state then transition.
    pub fn ids(&self) -> Vec<&'static str> {
        self.state_invariants
            .iter()
            .map(|i| i.id())
            .chain(self.transition_invariants.iter().map(|i| i.name()))
            .collect()
    }

    /// Number of registered invariants of both kinds.
    pub fn len(&self) -> usize {
        self.state_invariants.len() + self.transition_invariants.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Verify no two registered invariants share an id.
    ///
    /// Worth asserting rather than assuming: four epics (#2140, #2190, #2167,
    /// #2204) register into this harness independently, and duplicate ids
    /// would make a failure report point at the wrong module.
    pub fn assert_unique_ids(&self) -> Result<(), String> {
        let mut seen: Vec<&'static str> = self.ids();
        seen.sort_unstable();
        for pair in seen.windows(2) {
            if pair[0] == pair[1] {
                return Err(format!("duplicate invariant id registered: {}", pair[0]));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::{Address, Balance};

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    /// A state that satisfies everything the defaults check.
    fn healthy_state() -> State {
        let mut state = State::new();
        let acct = state.create_account(addr(1), Balance::new(1_000));
        acct.spendable_balance = Balance::new(600);
        acct.stake_locked = Balance::new(400);
        state
    }

    #[test]
    fn defaults_pass_on_a_healthy_state() {
        let harness = InvariantHarness::with_defaults();
        assert_eq!(harness.check_all(&healthy_state()), Ok(()));
    }

    #[test]
    fn defaults_register_both_kinds() {
        let harness = InvariantHarness::with_defaults();
        assert!(!harness.is_empty(), "with_defaults must register something");
        let ids = harness.ids();
        assert!(
            ids.iter().any(|i| i.contains("INV-BAL-2")),
            "state invariants registered: {ids:?}"
        );
        assert!(
            ids.iter().any(|i| i.contains("INV-SLASH-1")),
            "transition invariants registered: {ids:?}"
        );
    }

    /// Duplicate ids would misattribute a failure to the wrong module.
    #[test]
    fn default_ids_are_unique() {
        InvariantHarness::with_defaults()
            .assert_unique_ids()
            .expect("shipped invariants must not share ids");
    }

    #[test]
    fn assert_unique_ids_catches_a_collision() {
        struct Dup;
        impl Invariant for Dup {
            fn id(&self) -> &'static str {
                "INV-BAL-2: Balance Accounting Identity"
            }
            fn check(&self, _: &State) -> Result<(), String> {
                Ok(())
            }
        }
        let harness = InvariantHarness::with_defaults().with(Box::new(Dup));
        assert!(harness.assert_unique_ids().is_err());
    }

    /// A deliberately corrupted state fails with the expected invariant ID.
    #[test]
    fn corrupted_balance_fails_with_expected_id() {
        let mut state = healthy_state();
        // Break the accounting identity: parts no longer sum to the total.
        state
            .accounts
            .get_mut(&addr(1))
            .expect("fixture account")
            .spendable_balance = Balance::new(599);

        let violations = InvariantHarness::with_defaults()
            .check_all(&state)
            .expect_err("corrupted state must fail");

        assert!(
            violations
                .iter()
                .any(|(id, _)| *id == "INV-BAL-2: Balance Accounting Identity"),
            "got {violations:?}"
        );
        // The detail must name the account, or an operator cannot act on it.
        let (_, detail) = violations
            .iter()
            .find(|(id, _)| id.contains("INV-BAL-2"))
            .expect("the balance violation");
        assert!(detail.contains(&addr(1).to_string()), "got: {detail}");
    }

    /// Aggregation: ALL violated invariants are reported, not just the first.
    #[test]
    fn check_all_aggregates_every_violation() {
        let mut state = healthy_state();
        state
            .accounts
            .get_mut(&addr(1))
            .expect("fixture account")
            .spendable_balance = Balance::new(1);
        state
            .governance
            .params
            .pop_liveness_policy
            .useful_work_activation_height = 42;

        let violations = InvariantHarness::with_defaults()
            .check_all(&state)
            .expect_err("doubly-corrupted state must fail");

        assert_eq!(
            violations.len(),
            2,
            "both violations must be reported, got {violations:?}"
        );
        let ids: Vec<&str> = violations.iter().map(|(id, _)| *id).collect();
        assert!(ids.iter().any(|i| i.contains("INV-BAL-2")), "got {ids:?}");
        assert!(ids.iter().any(|i| i.contains("INV-POP-1")), "got {ids:?}");
    }

    /// The extension path four other epics will use.
    #[test]
    fn a_new_invariant_needs_one_registration_line() {
        struct EpicInvariant;
        impl Invariant for EpicInvariant {
            fn id(&self) -> &'static str {
                "INV-EXAMPLE-1: Downstream Epic Invariant"
            }
            fn check(&self, state: &State) -> Result<(), String> {
                if state.accounts.is_empty() {
                    Err("no accounts".to_string())
                } else {
                    Ok(())
                }
            }
        }

        let harness = InvariantHarness::with_defaults().with(Box::new(EpicInvariant));
        assert!(harness.ids().iter().any(|i| i.contains("INV-EXAMPLE-1")));
        assert_eq!(harness.check_all(&healthy_state()), Ok(()));

        let violations = harness
            .check_all(&State::new())
            .expect_err("empty state must trip the registered invariant");
        assert!(
            violations
                .iter()
                .any(|(id, _)| id.contains("INV-EXAMPLE-1"))
        );
    }

    /// Transition invariants are reachable through the same harness.
    #[test]
    fn check_transition_runs_the_transition_invariants() {
        let harness = InvariantHarness::with_defaults();
        let pre = healthy_state();
        let mut post = pre.clone();
        // Create tokens from nothing across a transfer — INV-BAL-1's job.
        post.accounts
            .get_mut(&addr(1))
            .expect("fixture account")
            .balance = Balance::new(2_000);

        let transition = StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(2),
            amount: Balance::new(1),
            nonce: None,
            current_block: 1,
        };

        let violations = harness
            .check_transition(&pre, &post, &transition)
            .expect_err("minting across a transfer must be caught");
        assert!(
            violations.iter().any(|(id, _)| id.contains("INV-BAL-1")),
            "got {violations:?}"
        );
    }

    /// `all_invariants()` and the per-domain hooks are two lists of the same
    /// thing, so they can drift. Every transition invariant in the curated
    /// set must be reachable through the harness — either directly, or as a
    /// deliberately-subsuming state form sharing its id.
    ///
    /// Without this, moving registration into the domain modules would let a
    /// transition invariant silently stop running while `all_invariants()`
    /// still advertised it — the same omission bug one list over.
    #[test]
    fn every_curated_transition_invariant_is_reachable_through_the_harness() {
        let ids = InvariantHarness::with_defaults().ids();
        for inv in super::super::all_invariants() {
            assert!(
                ids.contains(&inv.name()),
                "{} is in all_invariants() but no domain hook registers it \
                 (nor a state form sharing its id); registered: {ids:?}",
                inv.name()
            );
        }
    }

    /// Each domain hook must actually register something — an empty hook is
    /// indistinguishable from a domain whose invariants were all forgotten.
    #[test]
    fn every_domain_hook_registers_at_least_one_invariant() {
        for (domain, register) in DOMAIN_REGISTRATIONS {
            let mut harness = InvariantHarness::new();
            register(&mut harness);
            assert!(
                !harness.is_empty(),
                "domain hook `{domain}` registered nothing"
            );
        }
    }

    /// The domain hooks must partition the defaults — no invariant registered
    /// twice by two hooks, and none left over.
    #[test]
    fn domain_hooks_account_for_every_default() {
        let total: usize = DOMAIN_REGISTRATIONS
            .iter()
            .map(|(_, register)| {
                let mut harness = InvariantHarness::new();
                register(&mut harness);
                harness.len()
            })
            .sum();
        assert_eq!(
            total,
            InvariantHarness::with_defaults().len(),
            "the per-domain hooks and with_defaults() disagree on the count"
        );
        InvariantHarness::with_defaults()
            .assert_unique_ids()
            .expect("domain hooks must not double-register an id");
    }

    #[test]
    fn an_empty_harness_passes_everything() {
        let harness = InvariantHarness::new();
        assert!(harness.is_empty());
        assert_eq!(harness.check_all(&State::new()), Ok(()));
    }
}
