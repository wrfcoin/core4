//! Balance-related consensus invariants.
//!
//! This module owns its harness registration: see [`register_invariants`] at
//! the bottom of the file. Adding a balance invariant is an edit here and
//! nowhere else (#2260).

use super::harness::{Invariant, InvariantHarness};
use super::{ConsensusInvariant, InvariantResult};
use crate::blockchain::state::{State, StateTransition};
use crate::common::types::Balance;

fn total_balance(state: &State) -> Balance {
    state.accounts.values().map(|a| a.balance).sum()
}

pub struct BalanceConservation;

impl ConsensusInvariant for BalanceConservation {
    fn name(&self) -> &'static str {
        "INV-BAL-1: Balance Conservation"
    }

    fn check(
        &self,
        pre: &State,
        post: &State,
        transition: &StateTransition,
    ) -> Option<InvariantResult> {
        match transition {
            StateTransition::BalanceTransfer { .. } => {
                let pre_total = total_balance(pre);
                let post_total = total_balance(post);
                if pre_total == post_total {
                    Some(InvariantResult::pass(self.name()))
                } else {
                    Some(InvariantResult::fail(
                        self.name(),
                        format!(
                            "Total balance changed: pre={}, post={}, delta={}",
                            pre_total,
                            post_total,
                            post_total.into_inner() as i128 - pre_total.into_inner() as i128,
                        ),
                    ))
                }
            },
            _ => None,
        }
    }
}

pub struct BalanceAccountingIdentity;

impl ConsensusInvariant for BalanceAccountingIdentity {
    fn name(&self) -> &'static str {
        "INV-BAL-2: Balance Accounting Identity"
    }

    fn check(
        &self,
        _pre: &State,
        post: &State,
        _transition: &StateTransition,
    ) -> Option<InvariantResult> {
        for (addr, acct) in &post.accounts {
            let expected = acct.stake_locked + acct.spendable_balance;
            if acct.balance != expected {
                return Some(InvariantResult::fail(
                    self.name(),
                    format!(
                        "Account {}: balance={} != stake_locked({}) + spendable_balance({})",
                        addr, acct.balance, acct.stake_locked, acct.spendable_balance,
                    ),
                ));
            }
        }
        Some(InvariantResult::pass(self.name()))
    }
}

pub struct StakeLockReleaseConservation;

impl ConsensusInvariant for StakeLockReleaseConservation {
    fn name(&self) -> &'static str {
        "INV-BAL-3: Stake Lock/Release Conservation"
    }

    fn check(
        &self,
        pre: &State,
        post: &State,
        transition: &StateTransition,
    ) -> Option<InvariantResult> {
        let address = match transition {
            StateTransition::LockToStake { address, .. } => address,
            StateTransition::ReleaseToSpendable { address, .. } => address,
            _ => return None,
        };

        let pre_balance = pre
            .accounts
            .get(address)
            .map(|a| a.balance)
            .unwrap_or(Balance::new(0));
        let post_balance = post
            .accounts
            .get(address)
            .map(|a| a.balance)
            .unwrap_or(Balance::new(0));

        if pre_balance == post_balance {
            Some(InvariantResult::pass(self.name()))
        } else {
            Some(InvariantResult::fail(
                self.name(),
                format!(
                    "Account {address}: balance changed from {pre_balance} to {post_balance} during lock/release",
                ),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// State form of INV-BAL-2, and this domain's harness registration.
// ---------------------------------------------------------------------------

/// INV-BAL-2 as a state predicate: every account's total must equal the sum
/// of its locked and spendable parts.
///
/// The transition form in [`BalanceAccountingIdentity`] only ever reads
/// `post`, so nothing is lost by evaluating it against a single state — and
/// doing so means it runs after every applied block rather than only when a
/// transition is being observed. It shares that invariant's id, and
/// [`register_invariants`] registers only this form in consequence.
pub struct BalanceAccountingIdentityState;

impl Invariant for BalanceAccountingIdentityState {
    fn id(&self) -> &'static str {
        "INV-BAL-2: Balance Accounting Identity"
    }

    fn check(&self, state: &State) -> Result<(), String> {
        for (addr, acct) in &state.accounts {
            let expected = acct.stake_locked + acct.spendable_balance;
            if acct.balance != expected {
                return Err(format!(
                    "account {addr}: balance={} != stake_locked({}) + spendable_balance({})",
                    acct.balance, acct.stake_locked, acct.spendable_balance,
                ));
            }
        }
        Ok(())
    }
}

/// Register every balance invariant with the harness (#2260).
///
/// This hook is owned by this module. Downstream epics adding a balance
/// invariant edit this function and no shared list, so two epics extending
/// two different domains cannot conflict with each other.
///
/// [`BalanceAccountingIdentity`] is registered only in its state form above:
/// identical id means identical property, the state form subsumes the
/// transition form, and registering both would double-report one violation
/// and trip `assert_unique_ids`.
pub fn register_invariants(harness: &mut InvariantHarness) {
    harness.register(Box::new(BalanceAccountingIdentityState));
    harness.register_transition(Box::new(BalanceConservation));
    harness.register_transition(Box::new(StakeLockReleaseConservation));
}

// ---------------------------------------------------------------------------
// INV-SUPPLY-1 — global supply conservation (#2097)
// ---------------------------------------------------------------------------

/// The three left-hand-side terms of the supply identity: how much value the
/// protocol has ever been *authorized* to bring into or take out of existence.
///
/// # Production does not use this
///
/// As of #2097 the issuance journal lives in consensus state
/// (`State::supply_genesis` and the mint/burn counters) and the production
/// path is [`State::supply_ledger`] + [`State::check_supply_conservation`],
/// called from `apply_block_finalization`. Those read the journal, so no
/// caller can manufacture a pass by supplying a convenient origin.
///
/// `SupplyOrigin` remains for tests and for offline analysis of a state whose
/// issuance history is supplied from outside — e.g. auditing a snapshot
/// against an independently-derived genesis figure.
///
/// [`State::supply_ledger`]: crate::blockchain::state::State::supply_ledger
/// [`State::check_supply_conservation`]: crate::blockchain::state::State::check_supply_conservation
///
/// # Why it was a separate argument
///
/// It has to be, today. The chain does not journal issuance: `State` has no
/// genesis-supply record, no mint log and no burn log. `State::total_minted_supply`
/// is named as though it were one, but it is just the sum of account balances —
/// a right-hand-side quantity. Rewards are credited straight into accounts, and
/// `governance.treasury_balance` is declared but never mutated.
///
/// So the identity cannot be closed from state alone, and pretending otherwise
/// would be worse than not checking: deriving the left side from the right side
/// makes `check_supply` tautological — it would pass on a chain that had just
/// minted a billion coins from nowhere.
///
/// The caller therefore supplies these from the genesis configuration and the
/// issuance journal. Until that journal exists, the honest position is that
/// **this chain cannot yet prove its own supply**; see the note on
/// [`SupplyLedger::from_state`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SupplyOrigin {
    /// Total value created in the genesis block.
    pub genesis_supply: u128,
    /// Cumulative value minted since genesis under an authorized rule
    /// (block rewards, emissions).
    pub authorized_minting: u128,
    /// Cumulative value destroyed since genesis under an authorized rule
    /// (fee burn, slashing burn).
    pub authorized_burning: u128,
}

/// A full accounting snapshot of where every unit of value is, and where it
/// came from.
///
/// The identity checked by [`check_supply`]:
///
/// ```text
/// genesis_supply + authorized_minting - authorized_burning
///     == circulating + locked + reserves + unclaimed_rewards
///        + shielded_pool + proposal_deposits + paymaster_deposits
/// ```
///
/// Every term on the right is a **place value can legitimately sit**. Omitting
/// one does not make the invariant stricter — it makes it wrong, because the
/// legitimate operation that moves value there reads as issued value
/// disappearing. Both `shielded_pool` and `proposal_deposits` were found
/// missing this way in review of #2250: each produced a false violation on a
/// valid transaction. `paymaster_deposits` (#2171) is the same shape of term
/// as `proposal_deposits`: value debited from an account and held in a
/// separate consensus-tracked pool rather than a plain account balance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SupplyLedger {
    pub genesis_supply: u128,
    pub authorized_minting: u128,
    pub authorized_burning: u128,
    pub circulating: u128,
    pub locked: u128,
    pub reserves: u128,
    pub unclaimed_rewards: u128,
    /// Transparent value held inside the shielded pool.
    ///
    /// A shielded deposit debits a transparent account and stores only a
    /// commitment string, so this value is invisible to any sum over accounts.
    /// It is **not** a place value can hide from the identity: it is issued
    /// value that has moved custody, and the identity must see it or a
    /// legitimate deposit reads as issued value vanishing.
    pub shielded_pool: u128,
    /// Governance proposal deposits, debited from proposers and held in the
    /// proposal records until each proposal resolves.
    pub proposal_deposits: u128,
    /// Paymaster stakes (#2171): value debited from a paymaster's account
    /// and held in `State::paymasters` until withdrawn (after its unstake
    /// delay) or spent sponsoring an operation (in which case it moves to
    /// `authorized_burning` instead — see `paymaster.rs`'s module docs on
    /// why a sponsored fee is modeled as burned).
    pub paymaster_deposits: u128,
}

/// A breach of the supply identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupplyViolation {
    /// The two sides of the identity disagree.
    Imbalance {
        lhs: u128,
        rhs: u128,
        /// `rhs - lhs`. **Positive** means more value exists than was ever
        /// issued — value was created from nowhere. **Negative** means issued
        /// value has vanished from the accounting.
        delta: i128,
    },
    /// Checked arithmetic failed while evaluating the identity. Covers both
    /// directions: a sum exceeding `u128`, and `authorized_burning` exceeding
    /// total issuance. Never a wrapped or saturated value — a supply check
    /// that silently wraps is an exploit, not a check.
    Overflow { field: &'static str },
}

impl core::fmt::Display for SupplyViolation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SupplyViolation::Imbalance { lhs, rhs, delta } => write!(
                f,
                "INV-SUPPLY-1 violated: issued({lhs}) != held({rhs}), delta={delta} \
                 ({} value)",
                if *delta > 0 { "excess" } else { "missing" },
            ),
            SupplyViolation::Overflow { field } => write!(
                f,
                "INV-SUPPLY-1 could not be evaluated: checked arithmetic failed at `{field}`",
            ),
        }
    }
}

impl std::error::Error for SupplyViolation {}

/// Evaluate the global supply-conservation identity.
///
/// All arithmetic is checked. An overflow or underflow is a
/// [`SupplyViolation::Overflow`], never a wrapped value.
pub fn check_supply(l: &SupplyLedger) -> Result<(), SupplyViolation> {
    let issued =
        l.genesis_supply
            .checked_add(l.authorized_minting)
            .ok_or(SupplyViolation::Overflow {
                field: "genesis_supply + authorized_minting",
            })?;
    // Underflow here means more has been burned than was ever issued, which is
    // itself a supply violation — surfaced as an arithmetic failure rather than
    // wrapped into an enormous positive number.
    let lhs = issued
        .checked_sub(l.authorized_burning)
        .ok_or(SupplyViolation::Overflow {
            field: "authorized_burning",
        })?;

    // Single source of truth for the right-hand side. This used to re-sum the
    // terms inline, duplicating `held()` — so adding `shielded_pool` and
    // `proposal_deposits` to one of them left the other silently short, which
    // is precisely the arithmetic this invariant exists to catch. One sum, one
    // place to add a term.
    let rhs = l.held()?;

    if lhs == rhs {
        return Ok(());
    }

    // Signed delta without wrapping: the gap can exceed i128::MAX in principle.
    let delta = if rhs >= lhs {
        i128::try_from(rhs - lhs).map_err(|_| SupplyViolation::Overflow { field: "delta" })?
    } else {
        i128::try_from(lhs - rhs)
            .map(|d| -d)
            .map_err(|_| SupplyViolation::Overflow { field: "delta" })?
    };

    Err(SupplyViolation::Imbalance { lhs, rhs, delta })
}

impl SupplyLedger {
    /// Build a ledger by reading the right-hand-side terms out of live state.
    ///
    /// Reads, from the real stores:
    ///
    /// | Term | Source |
    /// |---|---|
    /// | `circulating` | sum of `Account::spendable_balance` |
    /// | `locked` | sum of `Account::stake_locked` |
    /// | `reserves` | `governance.treasury_balance` |
    /// | `unclaimed_rewards` | `Account::earned_balance_total` not yet reflected in `balance` |
    ///
    /// The left-hand side comes from `origin` — see [`SupplyOrigin`] for why it
    /// cannot come from `State`.
    ///
    /// # Known gap
    ///
    /// `unclaimed_rewards` is **structurally zero** on this chain: rewards are
    /// credited directly to `Account::balance` when applied, so there is no
    /// pending-reward pool to read. The term is kept in the identity because it
    /// is part of the accounting model and a future deferred-reward mechanism
    /// must slot into it rather than quietly unbalance the equation.
    ///
    /// `reserves` reads `governance.treasury_balance`, which is currently
    /// declared but never mutated, so it is also zero in practice.
    pub fn from_state(state: &State, origin: SupplyOrigin) -> Result<Self, SupplyViolation> {
        let mut circulating: u128 = 0;
        let mut locked: u128 = 0;

        for account in state.accounts.values() {
            circulating = circulating
                .checked_add(account.spendable_balance.into_inner())
                .ok_or(SupplyViolation::Overflow {
                    field: "circulating",
                })?;
            locked = locked
                .checked_add(account.stake_locked.into_inner())
                .ok_or(SupplyViolation::Overflow { field: "locked" })?;
        }

        let mut proposal_deposits: u128 = 0;
        for record in state.governance.proposals.values() {
            proposal_deposits = proposal_deposits
                .checked_add(record.deposit.into_inner())
                .ok_or(SupplyViolation::Overflow {
                    field: "proposal_deposits",
                })?;
        }

        let mut paymaster_deposits: u128 = 0;
        for account in state.paymasters.values() {
            paymaster_deposits = paymaster_deposits
                .checked_add(account.deposit)
                .ok_or(SupplyViolation::Overflow {
                    field: "paymaster_deposits",
                })?;
        }

        Ok(Self {
            genesis_supply: origin.genesis_supply,
            authorized_minting: origin.authorized_minting,
            authorized_burning: origin.authorized_burning,
            circulating,
            locked,
            reserves: u128::from(state.governance.treasury_balance),
            // No deferred-reward pool exists — see the doc comment above.
            unclaimed_rewards: 0,
            shielded_pool: state.shielded_pool,
            proposal_deposits,
            paymaster_deposits,
        })
    }

    /// Total value the ledger says currently exists (the right-hand side).
    /// Sum of every place value can legitimately sit.
    ///
    /// The **only** place the right-hand side is computed — `check_supply`
    /// calls this rather than re-summing. Overflow is attributed to the term
    /// that broke it, so a report names something actionable rather than just
    /// "the sum".
    pub fn held(&self) -> Result<u128, SupplyViolation> {
        let mut total = self.circulating;
        for (field, term) in [
            ("circulating + locked", self.locked),
            ("reserves", self.reserves),
            ("unclaimed_rewards", self.unclaimed_rewards),
            ("shielded_pool", self.shielded_pool),
            ("proposal_deposits", self.proposal_deposits),
            ("paymaster_deposits", self.paymaster_deposits),
        ] {
            total = total
                .checked_add(term)
                .ok_or(SupplyViolation::Overflow { field })?;
        }
        Ok(total)
    }

    /// Convenience: [`check_supply`] against this ledger.
    pub fn check(&self) -> Result<(), SupplyViolation> {
        check_supply(self)
    }
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
    fn balance_conservation_holds_on_valid_transfer() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(100));
        pre.create_account(addr(2), Balance::new(50));

        let transition = StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(2),
            amount: Balance::new(30),
            nonce: Some(0),
            current_block: 0,
        };

        let mut post = pre.clone();
        post.apply_transition(transition.clone()).unwrap();

        let result = BalanceConservation.check(&pre, &post, &transition).unwrap();
        assert!(result.passed, "Expected pass: {}", result.detail);
    }

    #[test]
    fn balance_conservation_detects_violation() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(100));
        pre.create_account(addr(2), Balance::new(50));

        let transition = StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(2),
            amount: Balance::new(30),
            nonce: Some(0),
            current_block: 0,
        };

        let mut post = pre.clone();
        post.apply_transition(transition.clone()).unwrap();
        post.accounts.get_mut(&addr(2)).unwrap().balance += Balance::new(1);

        let result = BalanceConservation.check(&pre, &post, &transition).unwrap();
        assert!(!result.passed, "Expected failure");
        assert!(result.detail.contains("delta="));
    }

    #[test]
    fn accounting_identity_holds_after_reward() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(0));
        pre.set_stake_required(addr(1), Balance::new(50));

        let transition = StateTransition::ApplyReward {
            address: addr(1),
            amount: Balance::new(70),
            reason: "test".to_string(),
        };

        let mut post = pre.clone();
        post.apply_transition(transition.clone()).unwrap();

        let result = BalanceAccountingIdentity
            .check(&pre, &post, &transition)
            .unwrap();
        assert!(result.passed, "Expected pass: {}", result.detail);
    }

    #[test]
    fn accounting_identity_detects_violation() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(100));

        let transition = StateTransition::LockToStake {
            address: addr(1),
            amount: Balance::new(30),
        };

        let mut post = pre.clone();
        post.apply_transition(transition.clone()).unwrap();
        post.accounts.get_mut(&addr(1)).unwrap().stake_locked += Balance::new(5);

        let result = BalanceAccountingIdentity
            .check(&pre, &post, &transition)
            .unwrap();
        assert!(!result.passed, "Expected failure");
    }

    #[test]
    fn lock_release_preserves_balance() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(100));

        let lock = StateTransition::LockToStake {
            address: addr(1),
            amount: Balance::new(40),
        };

        let mut post = pre.clone();
        post.apply_transition(lock.clone()).unwrap();

        let result = StakeLockReleaseConservation
            .check(&pre, &post, &lock)
            .unwrap();
        assert!(result.passed, "Expected pass: {}", result.detail);
    }

    #[test]
    fn expected_fail_transfer_creates_tokens_from_nothing() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(100));
        pre.create_account(addr(2), Balance::new(50));

        let transition = StateTransition::BalanceTransfer {
            from: addr(1),
            to: addr(2),
            amount: Balance::new(30),
            nonce: Some(0),
            current_block: 0,
        };

        // Simulate a bug: receiver gets tokens but sender is unchanged
        let mut post = pre.clone();
        let receiver = post.accounts.get_mut(&addr(2)).unwrap();
        receiver.balance += Balance::new(30);
        receiver.spendable_balance += Balance::new(30);

        let result = BalanceConservation.check(&pre, &post, &transition).unwrap();
        assert!(
            !result.passed,
            "Invariant should have caught token creation from nothing"
        );

        let result = BalanceAccountingIdentity
            .check(&pre, &post, &transition)
            .unwrap();
        assert!(
            result.passed,
            "Accounting identity should hold for consistent corruption"
        );
    }

    #[test]
    fn expected_fail_lock_changes_balance() {
        let mut pre = State::new();
        pre.create_account(addr(1), Balance::new(100));

        let lock = StateTransition::LockToStake {
            address: addr(1),
            amount: Balance::new(30),
        };

        let mut post = pre.clone();
        post.apply_transition(lock.clone()).unwrap();
        post.accounts.get_mut(&addr(1)).unwrap().balance += Balance::new(10);

        let result = StakeLockReleaseConservation
            .check(&pre, &post, &lock)
            .unwrap();
        assert!(
            !result.passed,
            "Invariant should catch balance change during lock"
        );
    }

    #[test]
    fn lock_release_with_missing_account_does_not_panic() {
        // Neither pre nor post state contains this address.
        // StakeLockReleaseConservation uses unwrap_or(0) for missing accounts,
        // so pre_balance == post_balance == 0 → pass.  Must not panic.
        let pre = State::new();
        let post = State::new();

        let transition = StateTransition::LockToStake {
            address: addr(42),
            amount: Balance::new(10),
        };

        let result = StakeLockReleaseConservation.check(&pre, &post, &transition);
        assert!(
            result.is_some(),
            "invariant should return Some for LockToStake transition"
        );
        assert!(
            result.unwrap().passed,
            "missing account treated as zero balance should pass conservation check"
        );
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
        fn balance_conservation_holds_for_valid_transfers(
            bal1 in 1u128..100_000,
            bal2 in 0u128..100_000,
            transfer_pct in 1u128..100,
        ) {
            let amount = (bal1 * transfer_pct / 100).max(1).min(bal1);
            let mut pre = State::new();
            pre.create_account(addr(1), Balance::new(bal1));
            pre.create_account(addr(2), Balance::new(bal2));

            let transition = StateTransition::BalanceTransfer {
                from: addr(1),
                to: addr(2),
                amount: Balance::new(amount),
                nonce: Some(0),
                current_block: 0,
            };

            let mut post = pre.clone();
            post.apply_transition(transition.clone()).unwrap();

            let result = BalanceConservation.check(&pre, &post, &transition).unwrap();
            prop_assert!(result.passed, "Valid transfer should conserve total balance");
        }
    }
}

// ---------------------------------------------------------------------------
// INV-SUPPLY-1 as a stateful invariant
// ---------------------------------------------------------------------------

/// INV-SUPPLY-1 in the form [`super::StatefulInvariant`] expects, so it is
/// polled and halted on by the **same** supervisor as INV-FINALITY-1 (#2096 /
/// PR #2246) instead of growing a second, weaker enforcement mechanism.
///
/// # Why stateful rather than a transition invariant
///
/// The identity's left-hand side is an *issuance journal* — genesis baseline
/// plus cumulative authorized mint and burn. No single `State` snapshot records
/// it, which is exactly the criterion [`super::StatefulInvariant`] exists for.
/// That also settles a real objection raised in review of #2250: the journal is
/// not in `state_root_material` and is not carried by snapshot sync, so two
/// honest nodes can hold the same committed state root with different journals.
///
/// A predicate that **gates consensus** cannot depend on uncommitted state —
/// nodes would accept or reject differently on identical committed input. A
/// node-local safety **detector** can, and that is what this is: it observes,
/// latches, and takes *this node* out of service. It never changes which blocks
/// this node considers valid, so it cannot fork the network. Committing the
/// journal to consensus remains a separate, deliberate hard-fork decision.
///
/// # Latching
///
/// Value created from nothing is not undone by later good behaviour, so the
/// violation latches and `check` keeps returning `Err` — that is what lets a
/// supervisor polling every few seconds catch a violation that happened
/// between two polls.
#[derive(Clone, Default, Debug)]
pub struct SharedSupplyInvariant {
    violation: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl SharedSupplyInvariant {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a violation observed at finalization. First one wins: the
    /// earliest observation is the one closest to the cause.
    pub fn observe_violation(&self, detail: String) {
        let mut guard = self.locked();
        if guard.is_none() {
            *guard = Some(detail);
        }
    }

    /// Latched violation, if any.
    pub fn violation(&self) -> Option<String> {
        self.locked().clone()
    }

    /// Recover from poisoning rather than propagate it — a panic in an
    /// unrelated task must not silently take the supply check offline.
    fn locked(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.violation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl super::StatefulInvariant for SharedSupplyInvariant {
    fn id(&self) -> &'static str {
        "INV-SUPPLY-1: Global Supply Conservation"
    }

    fn check(&self) -> Result<(), String> {
        match self.violation() {
            Some(detail) => Err(detail),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod supply_tests {
    use super::*;
    use crate::common::types::Address;

    /// A ledger that balances: 1000 issued at genesis, 1000 held.
    fn balanced() -> SupplyLedger {
        SupplyLedger {
            genesis_supply: 1_000,
            authorized_minting: 0,
            authorized_burning: 0,
            circulating: 700,
            locked: 200,
            reserves: 100,
            unclaimed_rewards: 0,
            shielded_pool: 0,
            proposal_deposits: 0,
            paymaster_deposits: 0,
        }
    }

    /// genesis: the identity holds at genesis with nothing minted or burned.
    #[test]
    fn genesis() {
        let l = balanced();
        assert_eq!(check_supply(&l), Ok(()));
        assert_eq!(l.held().expect("no overflow"), 1_000);
    }

    /// mint-burn: minting then burning the same amount returns to balance.
    #[test]
    fn mint_burn() {
        // Mint 500 into circulation.
        let minted = SupplyLedger {
            authorized_minting: 500,
            circulating: 1_200,
            ..balanced()
        };
        assert_eq!(check_supply(&minted), Ok(()), "mint must stay balanced");

        // Burn the same 500 back out.
        let burned = SupplyLedger {
            authorized_minting: 500,
            authorized_burning: 500,
            ..balanced()
        };
        assert_eq!(check_supply(&burned), Ok(()), "burn must return to balance");

        // Burning without removing the value from circulation is a violation.
        let bad = SupplyLedger {
            authorized_minting: 500,
            authorized_burning: 500,
            circulating: 1_200,
            ..balanced()
        };
        let err = check_supply(&bad).expect_err("unbacked burn must be caught");
        assert!(matches!(err, SupplyViolation::Imbalance { delta: 500, .. }));
    }

    /// reward-accrual: value moving from reserves to unclaimed rewards keeps
    /// the identity, because it is a transfer between right-hand-side buckets.
    #[test]
    fn reward_accrual() {
        let l = SupplyLedger {
            reserves: 40,
            unclaimed_rewards: 60,
            ..balanced()
        };
        assert_eq!(
            check_supply(&l),
            Ok(()),
            "moving value between RHS buckets must not change the total"
        );

        // Accruing a reward without debiting anything creates value.
        let inflated = SupplyLedger {
            unclaimed_rewards: 60,
            ..balanced()
        };
        let err = check_supply(&inflated).expect_err("unbacked reward must be caught");
        assert!(matches!(err, SupplyViolation::Imbalance { delta: 60, .. }));
    }

    /// slashing: locked stake moving into reserves conserves supply.
    #[test]
    fn slashing() {
        let l = SupplyLedger {
            locked: 150,
            reserves: 150,
            ..balanced()
        };
        assert_eq!(check_supply(&l), Ok(()), "slash must move, not destroy");

        // Slashing that burns must also be recorded as authorized burning.
        let burn_slash = SupplyLedger {
            locked: 150,
            reserves: 100,
            authorized_burning: 50,
            ..balanced()
        };
        assert_eq!(check_supply(&burn_slash), Ok(()));
    }

    /// off-by-one: a single unit out, with a correctly signed delta.
    #[test]
    fn off_by_one() {
        // One unit too much value held: delta is POSITIVE (value from nowhere).
        let excess = SupplyLedger {
            circulating: 701,
            ..balanced()
        };
        assert_eq!(
            check_supply(&excess),
            Err(SupplyViolation::Imbalance {
                lhs: 1_000,
                rhs: 1_001,
                delta: 1,
            }),
        );

        // One unit missing: delta is NEGATIVE.
        let missing = SupplyLedger {
            circulating: 699,
            ..balanced()
        };
        assert_eq!(
            check_supply(&missing),
            Err(SupplyViolation::Imbalance {
                lhs: 1_000,
                rhs: 999,
                delta: -1,
            }),
        );
    }

    /// overflow: u128 saturation is reported, never wrapped.
    #[test]
    fn overflow() {
        let lhs_overflow = SupplyLedger {
            genesis_supply: u128::MAX,
            authorized_minting: 1,
            ..Default::default()
        };
        assert_eq!(
            check_supply(&lhs_overflow),
            Err(SupplyViolation::Overflow {
                field: "genesis_supply + authorized_minting",
            }),
        );

        let rhs_overflow = SupplyLedger {
            circulating: u128::MAX,
            locked: 1,
            ..Default::default()
        };
        assert_eq!(
            check_supply(&rhs_overflow),
            Err(SupplyViolation::Overflow {
                field: "circulating + locked",
            }),
        );

        let reserves_overflow = SupplyLedger {
            circulating: u128::MAX,
            reserves: 1,
            ..Default::default()
        };
        assert_eq!(
            check_supply(&reserves_overflow),
            Err(SupplyViolation::Overflow { field: "reserves" }),
        );

        let rewards_overflow = SupplyLedger {
            circulating: u128::MAX,
            unclaimed_rewards: 1,
            ..Default::default()
        };
        assert_eq!(
            check_supply(&rewards_overflow),
            Err(SupplyViolation::Overflow {
                field: "unclaimed_rewards",
            }),
        );
    }

    /// Burning more than was ever issued is an arithmetic failure, not a
    /// wrapped-around enormous positive supply.
    #[test]
    fn burning_more_than_issued_never_wraps() {
        let l = SupplyLedger {
            genesis_supply: 100,
            authorized_burning: 101,
            ..Default::default()
        };
        assert_eq!(
            check_supply(&l),
            Err(SupplyViolation::Overflow {
                field: "authorized_burning",
            }),
        );
    }

    /// All seven terms participate: changing any single one alone breaks the
    /// identity. Guards against a future refactor dropping a term.
    #[test]
    fn every_term_is_load_bearing() {
        let base = balanced();
        assert_eq!(check_supply(&base), Ok(()));

        let mutations: [(&str, SupplyLedger); 10] = [
            (
                "genesis_supply",
                SupplyLedger {
                    genesis_supply: base.genesis_supply + 1,
                    ..base
                },
            ),
            (
                "authorized_minting",
                SupplyLedger {
                    authorized_minting: 1,
                    ..base
                },
            ),
            (
                "authorized_burning",
                SupplyLedger {
                    authorized_burning: 1,
                    ..base
                },
            ),
            (
                "circulating",
                SupplyLedger {
                    circulating: base.circulating + 1,
                    ..base
                },
            ),
            (
                "locked",
                SupplyLedger {
                    locked: base.locked + 1,
                    ..base
                },
            ),
            (
                "reserves",
                SupplyLedger {
                    reserves: base.reserves + 1,
                    ..base
                },
            ),
            (
                "unclaimed_rewards",
                SupplyLedger {
                    unclaimed_rewards: 1,
                    ..base
                },
            ),
            (
                "shielded_pool",
                SupplyLedger {
                    shielded_pool: 1,
                    ..base
                },
            ),
            (
                "proposal_deposits",
                SupplyLedger {
                    proposal_deposits: 1,
                    ..base
                },
            ),
            (
                "paymaster_deposits",
                SupplyLedger {
                    paymaster_deposits: 1,
                    ..base
                },
            ),
        ];

        for (term, ledger) in mutations {
            assert!(
                check_supply(&ledger).is_err(),
                "perturbing `{term}` alone must break the identity — it is not being checked"
            );
        }
    }

    /// `from_state` reads the real balance and stake stores.
    #[test]
    fn from_state_reads_real_stores() {
        let mut state = State::new();
        for (i, (spendable, staked)) in [(300u128, 50u128), (400, 150)].iter().enumerate() {
            let addr = Address([i as u8 + 1; 20]);
            let acct = state.create_account(addr, Balance::new(spendable + staked));
            acct.spendable_balance = Balance::new(*spendable);
            acct.stake_locked = Balance::new(*staked);
        }
        state.governance.treasury_balance = 100;

        let origin = SupplyOrigin {
            genesis_supply: 1_000,
            authorized_minting: 0,
            authorized_burning: 0,
        };
        let ledger = SupplyLedger::from_state(&state, origin).expect("no overflow");

        assert_eq!(ledger.circulating, 700, "sum of spendable_balance");
        assert_eq!(ledger.locked, 200, "sum of stake_locked");
        assert_eq!(ledger.reserves, 100, "governance.treasury_balance");
        assert_eq!(ledger.genesis_supply, 1_000, "carried from origin");
        assert_eq!(check_supply(&ledger), Ok(()));
    }

    /// A live state that has drifted from its declared issuance is caught.
    #[test]
    fn from_state_detects_drift_against_declared_issuance() {
        let mut state = State::new();
        let acct = state.create_account(Address([9; 20]), Balance::new(5_000));
        acct.spendable_balance = Balance::new(5_000);
        acct.stake_locked = Balance::new(0);

        // Genesis said 1000; state holds 5000.
        let origin = SupplyOrigin {
            genesis_supply: 1_000,
            ..Default::default()
        };
        let ledger = SupplyLedger::from_state(&state, origin).expect("no overflow");

        assert_eq!(
            check_supply(&ledger),
            Err(SupplyViolation::Imbalance {
                lhs: 1_000,
                rhs: 5_000,
                delta: 4_000,
            }),
            "4000 units exist that were never issued"
        );
    }

    /// An empty state holds nothing, so it balances only against zero issuance.
    #[test]
    fn from_state_on_empty_state() {
        let state = State::new();
        let ledger =
            SupplyLedger::from_state(&state, SupplyOrigin::default()).expect("no overflow");
        assert_eq!(ledger.held().expect("no overflow"), 0);
        assert_eq!(check_supply(&ledger), Ok(()));
    }

    #[test]
    fn violation_messages_are_actionable() {
        let excess = check_supply(&SupplyLedger {
            circulating: 701,
            ..balanced()
        })
        .expect_err("imbalance");
        let text = excess.to_string();
        assert!(text.contains("INV-SUPPLY-1"), "got: {text}");
        assert!(text.contains("excess"), "got: {text}");

        let missing = check_supply(&SupplyLedger {
            circulating: 699,
            ..balanced()
        })
        .expect_err("imbalance");
        assert!(missing.to_string().contains("missing"));
    }
}
