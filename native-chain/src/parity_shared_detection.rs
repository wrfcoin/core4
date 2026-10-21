//! What the shared/native gate can actually establish — and a detection gate
//! for the invariants it was claiming to protect (#2302).
//!
//! # Why this module exists
//!
//! `parity_shared.rs` compares `native::X` against `shared::X` for five
//! modules. Those comparisons **cannot fail**, and not by accident:
//! `native-chain/src/{accounts,identity,staking,weather,invariants}/mod.rs` are
//! one-line `pub use` re-exports of the shared crate, so both sides of every
//! comparison resolve to the *same item*. `git log --follow` on all five
//! returns exactly one commit — `0ef800231`, the same commit that added the
//! parity workflow and `parity_shared.rs`. The aliases were created **by** the
//! parity work; there was never a native implementation to diverge from.
//!
//! That makes "make them diverge" the wrong fix. A second implementation of
//! consensus code, written solely so a gate has something to compare, is worse
//! than the gate. What this module does instead is split the claim in two:
//!
//! 1. [`alias_identity`] asserts the re-export relationship still holds — that
//!    nobody has forked a copy. That is a real property, and the one the
//!    `native::`-vs-`shared::` comparisons were gesturing at.
//! 2. [`invariant_detection`] gates the property the old suite was blind to:
//!    that each consensus invariant still **detects** its violation.
//!
//! # The blindness this closes
//!
//! `parity_shared.rs` pins `INVARIANTS_SEMANTICS_BASELINE` as `(name, passed)`
//! pairs over a scenario where every invariant **passes**. So it catches an
//! invariant that starts *falsely failing*, and is blind to one that **stops
//! detecting**. For a safety invariant that is the dangerous direction: a check
//! silently disabled in production reads as green.
//!
//! Demonstrated on `main` before this module existed — mutating
//! `shared-protocol/src/invariants/rewards.rs` to
//!
//! ```text
//! if false && post_acct.earned_balance_total < pre_earned {
//! ```
//!
//! left all three commands in `rust-shared-native-parity.yml` green, while
//! renaming the same invariant turned them red. Catalogue drift was gated;
//! behavioural regression was not.
//!
//! [`invariant_detection`] closes that by pinning, for every invariant in
//! `all_invariants()`, a fixture that must make it **fail**. The `if false &&`
//! mutation now turns that fixture green, which the gate reports as a
//! regression. See `invariant_detection::expected_detections`.
//!
//! # What this module does NOT establish
//!
//! It does not establish behavioural parity between two implementations,
//! because there are not two implementations. See the module-level note in
//! [`alias_identity`] for the precise claim.

/// Assert that the five "parity-critical" native modules are still re-exports
/// of the shared crate rather than forked copies.
///
/// # The claim
///
/// This is **not** a parity check. It is an *aliasing* check, and the
/// distinction is the whole point of #2302: parity between two paths to one
/// item is vacuous, while "these two paths still resolve to one item" is a
/// property that can genuinely be violated — by someone replacing a `pub use`
/// with a local definition, which is exactly how a silent fork starts.
///
/// # Why `TypeId` and not a structural comparison
///
/// A field-by-field or serialization comparison would pass against an
/// identically-shaped fork, which is the realistic failure mode: someone copies
/// the struct to "customise it later". `TypeId` distinguishes them, because two
/// structurally identical types defined in different modules have different
/// ids.
///
/// # What this adds over the compiler — measured, not assumed
///
/// Forking a type is often already a **compile error**, because
/// `parity_shared.rs` passes native values into shared functions, so the two
/// must be the same type. That is worth knowing before trusting this module,
/// so each module was probed by replacing its `pub use` with an
/// identically-shaped local copy:
///
/// | Module | Fork probe result |
/// |---|---|
/// | `accounts` | **compile error** — `ValidationPipeline::set_policy` wants the shared `SmartAccountPolicy` |
/// | `weather` | **compile error** — `validate_weather_observation_with_temporal_context` wants the shared observation type |
/// | `invariants` | **compiles cleanly** — `invariants_semantics_parity` projects results to `(String, bool)` before comparing, so the types never meet. Only this module catches it |
///
/// So the compile-time coupling is real but **partial, accidental, and load-
/// bearing on a suite that is itself vacuous**. If someone acts on #2302 by
/// deleting the `native::`-vs-`shared::` comparisons — the obvious next step —
/// that coupling disappears silently and nothing replaces it. This module is
/// the explicit, documented version of a guarantee the tree currently gets by
/// accident.
#[cfg(test)]
mod alias_identity {
    use crate as native;
    use std::any::TypeId;
    use wrfcoin_shared_protocol as shared;

    /// One row per parity-critical module. Adding a module to
    /// `PARITY_CRITICAL_MODULES` in `parity_shared.rs` without adding a row
    /// here is caught by [`every_parity_critical_module_has_an_alias_assertion`].
    ///
    /// Each closure returns `(native TypeId, shared TypeId)` for a type the
    /// module re-exports. One representative type per module is sufficient: a
    /// fork replaces the whole `pub use` line, so any type under it moves.
    fn alias_rows() -> Vec<(&'static str, TypeId, TypeId)> {
        vec![
            (
                "accounts",
                TypeId::of::<native::accounts::abstraction::SmartAccountPolicy>(),
                TypeId::of::<shared::accounts::abstraction::SmartAccountPolicy>(),
            ),
            (
                "identity",
                TypeId::of::<native::identity::registry::ContributorIdentity>(),
                TypeId::of::<shared::identity::registry::ContributorIdentity>(),
            ),
            (
                "staking",
                TypeId::of::<native::staking::liquid_staking::LiquidStakingPool>(),
                TypeId::of::<shared::staking::liquid_staking::LiquidStakingPool>(),
            ),
            (
                "weather",
                TypeId::of::<native::weather::validation::WeatherObservation>(),
                TypeId::of::<shared::weather::validation::WeatherObservation>(),
            ),
            (
                "invariants",
                TypeId::of::<native::invariants::InvariantResult>(),
                TypeId::of::<shared::invariants::InvariantResult>(),
            ),
        ]
    }

    /// The gate: every parity-critical native module still resolves to the
    /// shared implementation.
    ///
    /// **Probe for this test.** Replace the `pub use` in any of
    /// `native-chain/src/{accounts,identity,staking,weather,invariants}/mod.rs`
    /// with a local definition of the same shape. This test must turn red.
    #[test]
    fn parity_critical_modules_are_reexports_not_forks() {
        for (module, native_id, shared_id) in alias_rows() {
            assert_eq!(
                native_id, shared_id,
                "native-chain module '{module}' no longer re-exports the shared \
                 implementation — it has been forked. Two implementations of a \
                 consensus module diverge silently; if the fork is deliberate, \
                 delete this row and give '{module}' a real differential test \
                 against an independent oracle (core4#2302)."
            );
        }
    }

    /// Keep the alias rows in step with the module list `parity_shared.rs`
    /// declares, so a sixth parity-critical module cannot be added without
    /// either an alias assertion or a deliberate decision to omit one.
    #[test]
    fn every_parity_critical_module_has_an_alias_assertion() {
        let asserted: Vec<&str> = alias_rows().iter().map(|(m, _, _)| *m).collect();
        for module in crate::parity_shared::PARITY_CRITICAL_MODULES {
            assert!(
                asserted.contains(&module),
                "parity-critical module '{module}' has no alias assertion in \
                 parity_shared_detection::alias_identity. Either add one, or \
                 remove the module from PARITY_CRITICAL_MODULES (core4#2302)."
            );
        }
    }
}

/// Gate that every consensus invariant still **detects** its violation.
///
/// # The property
///
/// For each invariant in `shared::invariants::all_invariants()`, this module
/// pins a fixture that must make it **fail**. `parity_shared.rs` already pins
/// expected-pass outcomes and one expected-fail outcome (`EmissionCap`); this
/// extends the expected-fail side to every invariant, which is the side that
/// catches a silently-disabled check.
///
/// # Why expected-fail fixtures and not expected-pass
///
/// An invariant that stops enforcing still passes every expected-pass fixture.
/// Only a case that is *supposed to be caught* distinguishes "enforcing" from
/// "returning pass unconditionally".
#[cfg(test)]
mod invariant_detection {
    use shared::blockchain::state::{State, StateTransition};
    use shared::common::types::{Address, Balance};
    use wrfcoin_shared_protocol as shared;

    fn addr(seed: u8) -> Address {
        Address([seed; 20])
    }

    /// A violation fixture: a `(pre, post, transition)` triple that the named
    /// invariant must reject.
    struct Violation {
        /// Must match `ConsensusInvariant::name()` exactly.
        invariant: &'static str,
        /// Builds the state triple that violates it.
        build: fn() -> (State, State, StateTransition),
    }

    /// One violation per invariant in `all_invariants()`.
    ///
    /// **This list is the acceptance probe for core4#2302.** Disabling any
    /// invariant's detection — for example
    /// `if false && post_acct.earned_balance_total < pre_earned` in
    /// `shared-protocol/src/invariants/rewards.rs` — makes its fixture pass,
    /// and [`every_invariant_detects_its_violation`] reports that as a
    /// regression. Before this module existed, that mutation left the whole
    /// parity workflow green.
    fn expected_detections() -> Vec<Violation> {
        vec![
            Violation {
                invariant: "INV-BAL-1: Balance Conservation",
                // Guarded on BalanceTransfer; fails when total supply moves.
                build: || {
                    let mut pre = State::new();
                    pre.create_account(addr(1), Balance::new(100));
                    pre.create_account(addr(2), Balance::new(100));
                    let mut post = pre.clone();
                    post.create_account(addr(2), Balance::new(0)).balance = Balance::new(500);
                    (
                        pre,
                        post,
                        StateTransition::BalanceTransfer {
                            from: addr(1),
                            to: addr(2),
                            amount: Balance::new(10),
                            nonce: None,
                            current_block: 1,
                        },
                    )
                },
            },
            Violation {
                invariant: "INV-BAL-2: Balance Accounting Identity",
                // Unguarded; fails when balance != stake_locked + spendable.
                build: || {
                    let pre = State::new();
                    let mut post = State::new();
                    let acct = post.create_account(addr(3), Balance::new(0));
                    acct.balance = Balance::new(100);
                    acct.stake_locked = Balance::new(0);
                    acct.spendable_balance = Balance::new(0);
                    (
                        pre,
                        post,
                        StateTransition::ApplyReward {
                            address: addr(3),
                            amount: Balance::new(1),
                            reason: "detection-probe".to_string(),
                        },
                    )
                },
            },
            Violation {
                invariant: "INV-BAL-3: Stake Lock/Release Conservation",
                // Guarded on LockToStake; fails when total balance moves
                // during a lock, which must only reshuffle within the account.
                build: || {
                    let mut pre = State::new();
                    pre.create_account(addr(4), Balance::new(100));
                    let mut post = pre.clone();
                    post.create_account(addr(4), Balance::new(0)).balance = Balance::new(40);
                    (
                        pre,
                        post,
                        StateTransition::LockToStake {
                            address: addr(4),
                            amount: Balance::new(60),
                        },
                    )
                },
            },
            Violation {
                invariant: "INV-SLASH-1: Slashing Bound",
                // Guarded on Slash; fails when the slashed account's balance
                // increases instead of decreasing.
                build: || {
                    let mut pre = State::new();
                    pre.create_account(addr(5), Balance::new(100));
                    let mut post = pre.clone();
                    post.create_account(addr(5), Balance::new(0)).balance = Balance::new(900);
                    (
                        pre,
                        post,
                        StateTransition::Slash {
                            address: addr(5),
                            amount: Balance::new(10),
                            reason: "detection-probe".to_string(),
                        },
                    )
                },
            },
            Violation {
                invariant: "INV-POP-1: Useful-Work Activation Non-Regression",
                // Unguarded; useful-work-only mode requires post height == 0.
                build: || {
                    let pre = State::new();
                    let mut post = State::new();
                    post.governance
                        .params
                        .pop_liveness_policy
                        .useful_work_activation_height = 42;
                    (
                        pre,
                        post,
                        StateTransition::ApplyReward {
                            address: addr(6),
                            amount: Balance::new(0),
                            reason: "detection-probe".to_string(),
                        },
                    )
                },
            },
            Violation {
                invariant: "INV-RWD-1: Reward Monotonicity",
                // earned_balance_total decreases. This is the exact case the
                // `if false &&` mutation stops detecting, and the reason this
                // module exists.
                build: || {
                    let mut pre = State::new();
                    pre.create_account(addr(7), Balance::new(100))
                        .earned_balance_total = Balance::new(50);
                    let mut post = pre.clone();
                    post.create_account(addr(7), Balance::new(0))
                        .earned_balance_total = Balance::new(10);
                    (
                        pre,
                        post,
                        StateTransition::ApplyReward {
                            address: addr(7),
                            amount: Balance::new(1),
                            reason: "detection-probe".to_string(),
                        },
                    )
                },
            },
            Violation {
                invariant: "INV-RWD-2: Emission Cap",
                // Reward far above the default per-transition emission cap.
                build: || {
                    let pre = State::new();
                    let post = State::new();
                    (
                        pre,
                        post,
                        StateTransition::ApplyReward {
                            address: addr(8),
                            amount: Balance::new(
                                shared::invariants::rewards::DEFAULT_EMISSION_CAP.saturating_add(1),
                            ),
                            reason: "detection-probe".to_string(),
                        },
                    )
                },
            },
        ]
    }

    /// **The acceptance gate for core4#2302.**
    ///
    /// Every invariant must reject its violation fixture. An invariant whose
    /// detection is disabled returns `pass` here and turns this test red.
    #[test]
    fn every_invariant_detects_its_violation() {
        let invariants = shared::invariants::all_invariants();
        let mut undetected = Vec::new();

        for violation in expected_detections() {
            let invariant = invariants
                .iter()
                .find(|inv| inv.name() == violation.invariant)
                .unwrap_or_else(|| {
                    panic!(
                        "violation fixture names '{}', which is not in \
                         all_invariants(). Registry and fixtures have drifted \
                         (core4#2302).",
                        violation.invariant
                    )
                });

            let (pre, post, transition) = (violation.build)();
            match invariant.check(&pre, &post, &transition) {
                Some(result) if !result.passed => { /* detected — correct */ },
                Some(result) => undetected.push(format!(
                    "{} PASSED its violation fixture (detail: {:?})",
                    violation.invariant, result.detail
                )),
                None => undetected.push(format!(
                    "{} returned None for its violation fixture",
                    violation.invariant
                )),
            }
        }

        assert!(
            undetected.is_empty(),
            "consensus invariant(s) no longer detect their violation — a check \
             has been disabled or weakened:\n  {}\n\nThis is the regression \
             class core4#2302 was filed for: the old parity suite pinned only \
             expected-PASS outcomes, so a silently-disabled invariant read as \
             green.",
            undetected.join("\n  ")
        );
    }

    /// Every invariant in the registry has a violation fixture.
    ///
    /// Without this, adding an invariant to `all_invariants()` would silently
    /// add an ungated one — the coarse-unit-of-identity pattern (core4#2284)
    /// that #2302 is an instance of.
    #[test]
    fn every_registered_invariant_has_a_violation_fixture() {
        let fixtures: Vec<&str> = expected_detections().iter().map(|v| v.invariant).collect();

        let missing: Vec<String> = shared::invariants::all_invariants()
            .iter()
            .map(|inv| inv.name().to_string())
            .filter(|name| !fixtures.contains(&name.as_str()))
            .collect();

        assert!(
            missing.is_empty(),
            "consensus invariant(s) have no violation fixture, so nothing \
             gates that they still detect anything:\n  {}\n\nAdd a fixture to \
             expected_detections() (core4#2302).",
            missing.join("\n  ")
        );
    }
}
