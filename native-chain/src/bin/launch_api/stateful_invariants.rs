//! The shipped stateful-invariant registry and the task that polls it (#2097).
//!
//! [`StatefulInvariantRegistry`] is the contract stateful invariants register
//! into, but a registry nobody polls enforces nothing. Before this module,
//! `launch_api` built a registry holding INV-FINALITY-1 and never called
//! `check_all()` anywhere in production — the poll existed only in unit tests.
//! INV-SUPPLY-1 (#2097) would have inherited exactly that: a latch that gets
//! set, logged, and ignored, which is the #2056 failure class in a new domain.
//!
//! This module owns three things, deliberately in one place:
//!
//! 1. [`build_shipped_registry`] — the ONE constructor the binary uses. A test
//!    can assert what the shipped node actually registers because there is a
//!    single function to assert about.
//! 2. [`assert_shipped_registry_complete`] — the boot-time gate. Every id in
//!    [`REQUIRED_STATEFUL_INVARIANTS`] must be present or the node refuses to
//!    start. A missing invariant is not a warning: a node running without its
//!    supply detector looks identical to one whose supply is conserved.
//! 3. [`run_stateful_invariant_supervisor`] — the production poll, which turns
//!    a latched violation into a read-only node and a process exit through the
//!    same [`quiesce_after_safety_violation`] halt INV-FINALITY-1 uses.
//!
//! [`quiesce_after_safety_violation`]: crate::finality_events::quiesce_after_safety_violation

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;
use wrfcoin_shared_protocol::invariants::StatefulInvariantRegistry;
use wrfcoin_shared_protocol::invariants::balance::SharedSupplyInvariant;
use wrfcoin_shared_protocol::invariants::finality::SharedFinalityInvariant;
use wrfcoin_shared_protocol::mempool::pool::Mempool;

/// Every stateful invariant the shipped node must have armed.
///
/// These are the full titled ids returned by
/// [`StatefulInvariant::id`](wrfcoin_shared_protocol::invariants::StatefulInvariant::id),
/// compared verbatim. Adding a stateful invariant means adding it here AND to
/// [`build_shipped_registry`]; the boot gate then makes forgetting either one a
/// startup failure rather than a silent hole.
pub const REQUIRED_STATEFUL_INVARIANTS: &[&str] = &[
    "INV-FINALITY-1: No Conflicting Finalization",
    "INV-SUPPLY-1: Global Supply Conservation",
];

/// How often the supervisor polls.
///
/// A latched violation is not undone by later good behaviour, so the interval
/// only bounds *detection latency*, not correctness. Five seconds is under one
/// block time on the testnet, so at most one further block is produced between
/// the breach and the halt.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Build the registry the node actually ships.
///
/// Takes the live handles by reference and registers **clones of those exact
/// instances**. That is the whole point: a `SharedSupplyInvariant` constructed
/// here instead of being the one wired into `State::supply_invariant` would
/// poll a latch nobody ever writes to and pass forever — the identical hazard
/// the INV-FINALITY-1 waiver in `invariants::registration_audit` calls out.
pub fn build_shipped_registry(
    finality: &SharedFinalityInvariant,
    supply: &SharedSupplyInvariant,
) -> StatefulInvariantRegistry {
    StatefulInvariantRegistry::new()
        .with(Arc::new(finality.clone()))
        .with(Arc::new(supply.clone()))
}

/// Boot gate: the shipped registry must hold every required invariant, once.
///
/// Returns the reason on failure rather than logging, so the caller decides
/// between refusing to start (production) and asserting (tests).
pub fn assert_shipped_registry_complete(
    registry: &StatefulInvariantRegistry,
) -> Result<(), String> {
    registry.assert_unique_ids()?;
    let ids = registry.ids();
    let missing: Vec<&str> = REQUIRED_STATEFUL_INVARIANTS
        .iter()
        .copied()
        .filter(|required| !ids.iter().any(|id| id == required))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "stateful invariant registry is incomplete: {missing:?} required but not \
             registered (armed: {ids:?}). A node whose safety detector is not armed \
             is indistinguishable from a node with nothing to detect."
        ));
    }
    Ok(())
}

/// Why the supervisor stopped. It only ever stops on a violation — the poll
/// loop is otherwise infinite — so this type is always fatal.
#[derive(Debug)]
#[must_use = "a stateful-invariant violation must halt the node, not be dropped"]
pub struct SupervisorExit {
    /// Every violation the final poll reported, not just the first.
    pub violations: Vec<(&'static str, String)>,
}

impl SupervisorExit {
    /// One line naming every violated invariant, for the halt log.
    ///
    /// Written as a fold rather than `map(..).collect::<Vec<_>>().join(..)`
    /// because `scripts/check_no_panic_on_input.py` counts an intermediate
    /// `collect` on this surface as unbounded-allocation risk. The bound here
    /// is the registry size, but the gate is right to not take that on trust,
    /// and the loop is one allocation instead of N + 1 either way.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        for (id, detail) in &self.violations {
            if !out.is_empty() {
                out.push_str("; ");
            }
            out.push_str(id);
            out.push_str(" — ");
            out.push_str(detail);
        }
        out
    }
}

/// Poll the registry until something breaks, then take the node out of service.
///
/// Returns `SupervisorExit` so the caller performs the process termination,
/// keeping `std::process::exit` at its single reviewable call site in `main.rs`
/// — the same split [`crate::finality_events::supervise_finality_consumer`]
/// uses, and what makes the halt testable without exiting the test binary.
///
/// The first tick of a `tokio::time::interval` completes immediately, so a
/// violation latched before this task starts is caught on the first poll rather
/// than one interval later.
pub async fn run_stateful_invariant_supervisor(
    registry: StatefulInvariantRegistry,
    partition_mode: Arc<RwLock<PartitionMode>>,
    mempool: Arc<RwLock<Mempool>>,
    poll_interval: Duration,
) -> SupervisorExit {
    log::info!(
        "stateful-invariant supervisor started: polling {:?} every {:?}",
        registry.ids(),
        poll_interval,
    );
    let mut tick = tokio::time::interval(poll_interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        if let Err(violations) = registry.check_all() {
            let exit = SupervisorExit { violations };
            crate::finality_events::quiesce_after_safety_violation(
                &partition_mode,
                &mempool,
                &exit.summary(),
                "stateful-invariant-supervisor",
            )
            .await;
            return exit;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_native_chain::config::settings::Config;
    use wrfcoin_shared_protocol::blockchain::state::State;
    use wrfcoin_shared_protocol::common::types::{Address, Balance};

    fn mempool() -> Arc<RwLock<Mempool>> {
        Arc::new(RwLock::new(Mempool::new(&Config::default())))
    }

    fn partition_mode() -> Arc<RwLock<PartitionMode>> {
        Arc::new(RwLock::new(PartitionMode::Normal))
    }

    /// The shipped registry — the one `main.rs` builds — contains INV-SUPPLY-1.
    ///
    /// This is the assertion the #2250 review asked for. Before it, the only
    /// evidence that INV-SUPPLY-1 was registered anywhere was prose in a
    /// `WaiverKind::OtherRegistry` reason, and prose cannot establish runtime
    /// registration.
    #[test]
    fn shipped_registry_arms_every_required_invariant() {
        let registry =
            build_shipped_registry(&SharedFinalityInvariant::new(), &SharedSupplyInvariant::new());
        assert_shipped_registry_complete(&registry).expect("shipped registry must be complete");
        for required in REQUIRED_STATEFUL_INVARIANTS {
            assert!(
                registry.ids().iter().any(|id| id == required),
                "{required} missing from shipped registry {:?}",
                registry.ids(),
            );
        }
    }

    /// The gate is capable of failing: drop INV-SUPPLY-1 and it must reject.
    ///
    /// A completeness check that passes on an incomplete registry is the
    /// #2302 shape — a gate comparing something to itself.
    #[test]
    fn completeness_gate_rejects_a_registry_missing_the_supply_invariant() {
        let finality_only = StatefulInvariantRegistry::new()
            .with(Arc::new(SharedFinalityInvariant::new()));
        let err = assert_shipped_registry_complete(&finality_only)
            .expect_err("a registry without INV-SUPPLY-1 must be rejected");
        assert!(err.contains("INV-SUPPLY-1"), "got: {err}");
    }

    /// End-to-end through the production surfaces: an unjournalled mint on a
    /// `State` whose `supply_invariant` is the shipped handle is seen by the
    /// shipped registry's `check_all()`.
    ///
    /// This is the wiring the review found missing — `State` handle → registry
    /// → poll — asserted on the same constructor `main.rs` calls.
    #[test]
    fn an_unjournalled_mint_reaches_the_shipped_registry() {
        let finality = SharedFinalityInvariant::new();
        let supply = SharedSupplyInvariant::new();
        let registry = build_shipped_registry(&finality, &supply);

        let mut state = State::new();
        // The one line `main.rs` runs on the loaded blockchain. Without it this
        // test fails with a clean `check_all()` on a state that has definitely
        // been robbed — which is precisely the production defect the #2250
        // review found.
        state.supply_invariant = Some(supply.clone());
        let addr = Address([0x11; 20]);
        state.create_account(addr, Balance::new(1_000));
        // Baseline adopts the 1_000 already held; identity holds.
        state.apply_block_finalization(1);
        assert_eq!(registry.check_all(), Ok(()), "baseline must be clean");

        // Value from nowhere: credit an account with no journal entry.
        if let Some(account) = state.get_account_mut(&addr) {
            account.balance = Balance::new(1_500);
            account.spendable_balance = Balance::new(1_500);
        }
        state.apply_block_finalization(2);

        let violations = registry
            .check_all()
            .expect_err("the shipped registry must report the unjournalled mint");
        assert_eq!(violations.len(), 1, "got: {violations:?}");
        assert_eq!(violations[0].0, "INV-SUPPLY-1: Global Supply Conservation");
        assert!(
            violations[0].1.contains("delta=500"),
            "the report must carry the signed delta; got: {}",
            violations[0].1,
        );
    }

    /// The supervisor does not merely observe: it forces the node read-only.
    ///
    /// Without this the violation is still swallowed — logged by a background
    /// task while the node keeps producing blocks.
    #[tokio::test]
    async fn supervisor_forces_read_only_on_a_supply_violation() {
        let supply = SharedSupplyInvariant::new();
        let registry = build_shipped_registry(&SharedFinalityInvariant::new(), &supply);
        supply.observe_violation("INV-SUPPLY-1 violated: issued(10000) != held(10250), delta=250".into());

        let mode = partition_mode();
        let pool = mempool();
        let exit = run_stateful_invariant_supervisor(
            registry,
            mode.clone(),
            pool.clone(),
            Duration::from_millis(10),
        )
        .await;

        assert_eq!(exit.violations.len(), 1);
        assert!(exit.summary().contains("delta=250"), "{}", exit.summary());
        assert_eq!(*mode.read().await, PartitionMode::ReadOnly);
        assert_eq!(pool.read().await.partition_mode(), PartitionMode::ReadOnly);
    }

    /// A clean registry keeps polling rather than returning — otherwise a
    /// supervisor that exited on its first quiet poll would leave the node
    /// unwatched while looking like it had run.
    #[tokio::test]
    async fn supervisor_keeps_polling_while_nothing_is_violated() {
        let registry =
            build_shipped_registry(&SharedFinalityInvariant::new(), &SharedSupplyInvariant::new());
        let result = tokio::time::timeout(
            Duration::from_millis(80),
            run_stateful_invariant_supervisor(
                registry,
                partition_mode(),
                mempool(),
                Duration::from_millis(5),
            ),
        )
        .await;
        assert!(result.is_err(), "supervisor must not return without a violation");
    }
}
