//! Formal consensus invariants and bounded model checker.
//!
//! This module defines machine-checkable safety invariants for the WRFCoin
//! consensus state machine.
//!
//! # Two kinds of invariant live here
//!
//! **Transition invariants** implement [`ConsensusInvariant`]: a pure
//! predicate over `(pre, post, transition)`, checked around every
//! `StateTransition`. `balance`, `slashing`, `rewards` and `pop` are these,
//! and [`all_invariants`] returns the registered set.
//!
//! **Stateful invariants** track a property that no single `State` snapshot
//! records, by observing events as they happen. [`finality`] is the first:
//! "have we ever finalized a different block at this height?" cannot be
//! answered from the current state, because the current state is exactly what
//! a fork rewrites. These own their own history, are fed from the code path
//! that produces the event, and expose a pure `check()` for polling.
//!
//! A stateful invariant is deliberately **not** in [`all_invariants`] — it has
//! no meaningful `(pre, post, transition)` reading. It registers instead into
//! [`StatefulInvariantRegistry`], which is the contract future stateful
//! invariants (economic, cross-chain, account, sensor) implement.

pub mod balance;
pub mod finality;
pub mod harness;
pub mod model_checker;
pub mod pop;
pub mod registration_audit;
pub mod rewards;
pub mod slashing;

use crate::blockchain::state::{State, StateTransition};

/// Result of checking a single invariant against a state transition.
#[derive(Debug, Clone)]
pub struct InvariantResult {
    pub invariant_name: &'static str,
    pub passed: bool,
    pub detail: String,
}

impl InvariantResult {
    pub fn pass(name: &'static str) -> Self {
        Self {
            invariant_name: name,
            passed: true,
            detail: String::new(),
        }
    }

    pub fn fail(name: &'static str, detail: String) -> Self {
        Self {
            invariant_name: name,
            passed: false,
            detail,
        }
    }
}

/// A consensus invariant is a pure predicate over pre-state, post-state,
/// and the transition that connected them.
pub trait ConsensusInvariant: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(
        &self,
        pre: &State,
        post: &State,
        transition: &StateTransition,
    ) -> Option<InvariantResult>;
}

/// Run all registered invariants against a single transition.
pub fn check_all_invariants(
    invariants: &[Box<dyn ConsensusInvariant>],
    pre: &State,
    post: &State,
    transition: &StateTransition,
) -> Vec<InvariantResult> {
    invariants
        .iter()
        .filter_map(|inv| inv.check(pre, post, transition))
        .collect()
}

// ---------------------------------------------------------------------------
// Stateful invariants
// ---------------------------------------------------------------------------

/// An invariant that tracks a property **no single `State` snapshot records**,
/// by observing events as they happen.
///
/// [`ConsensusInvariant`] cannot express these. "Have we ever finalized a
/// different block at this height?" is not a function of the current state —
/// a fork is precisely what rewrites that state. Such an invariant must own
/// its own history, be fed from the code path that produces the event, and
/// expose a pure check for polling.
///
/// # Implementing one
///
/// 1. Keep the mutable history behind a shared handle so the same instance can
///    be *fed* at its event site and *polled* by a registry. See
///    [`finality::SharedFinalityInvariant`] for the pattern — a `Mutex` around
///    the tracker, with `&self` methods.
/// 2. Give it a unique `INV-<DOMAIN>-<n>` id.
/// 3. `check` must be pure and must never panic — return `Err(String)`.
///    A panicking invariant takes down whatever is polling it and hides every
///    other violation in the same sweep.
/// 4. Register it with [`StatefulInvariantRegistry::register`].
///
/// # Latching
///
/// A safety violation is not undone by subsequent good behaviour, so
/// implementations should latch: once `check` returns `Err`, it should keep
/// doing so. That is what lets a caller which only polls periodically observe
/// a violation that happened between polls.
pub trait StatefulInvariant: Send + Sync {
    /// Unique identifier, e.g. `"INV-FINALITY-1: No Conflicting Finalization"`.
    fn id(&self) -> &'static str;

    /// `Ok(())` if no violation has been observed, else a description specific
    /// enough to act on. Pure — safe to call from a polling supervisor.
    fn check(&self) -> Result<(), String>;
}

/// Registry of stateful invariants, polled together.
///
/// Deliberately holds `Arc` rather than `Box`: a stateful invariant has to be
/// *shared* — fed at its event site, polled here — which a `Box` cannot do.
#[derive(Default, Clone)]
pub struct StatefulInvariantRegistry {
    invariants: Vec<std::sync::Arc<dyn StatefulInvariant>>,
}

impl StatefulInvariantRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an invariant. One line per invariant — that is the whole
    /// extension surface.
    pub fn register(&mut self, invariant: std::sync::Arc<dyn StatefulInvariant>) -> &mut Self {
        self.invariants.push(invariant);
        self
    }

    /// Builder form, for call-site registration.
    pub fn with(mut self, invariant: std::sync::Arc<dyn StatefulInvariant>) -> Self {
        self.invariants.push(invariant);
        self
    }

    /// Poll every registered invariant, reporting **all** violations rather
    /// than the first — when things break they usually break in several places
    /// at once, and stopping early turns one investigation into several.
    pub fn check_all(&self) -> Result<(), Vec<(&'static str, String)>> {
        let violations: Vec<(&'static str, String)> = self
            .invariants
            .iter()
            .filter_map(|inv| inv.check().err().map(|detail| (inv.id(), detail)))
            .collect();
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.invariants.iter().map(|i| i.id()).collect()
    }

    pub fn len(&self) -> usize {
        self.invariants.len()
    }

    pub fn is_empty(&self) -> bool {
        self.invariants.is_empty()
    }

    /// Reject duplicate ids: two invariants sharing one id makes a violation
    /// report point at the wrong module.
    pub fn assert_unique_ids(&self) -> Result<(), String> {
        let mut seen = self.ids();
        seen.sort_unstable();
        for pair in seen.windows(2) {
            if pair[0] == pair[1] {
                return Err(format!("duplicate stateful invariant id: {}", pair[0]));
            }
        }
        Ok(())
    }
}

/// Build the default set of all consensus invariants.
pub fn all_invariants() -> Vec<Box<dyn ConsensusInvariant>> {
    vec![
        Box::new(balance::BalanceConservation),
        Box::new(balance::BalanceAccountingIdentity),
        Box::new(balance::StakeLockReleaseConservation),
        Box::new(slashing::SlashingBound),
        Box::new(pop::UsefulWorkActivationNonRegression),
        Box::new(rewards::RewardMonotonicity),
        Box::new(rewards::EmissionCap::default()),
    ]
}
