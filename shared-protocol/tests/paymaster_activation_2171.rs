//! #2171 activation: `ConsensusFeature::Paymaster` scheduled at H flips the
//! `StateTransition::ApplyPaymasterAction` arm at exactly H, through the REAL
//! `State::apply_transition`. This is the arm-side coverage the in-crate
//! `paymaster_state_tests` cannot give: those helpers copy the arm and call
//! `paymaster::apply` directly, so they are blind to the arm's own feature gate
//! (N5) and its Stake account-debit (N6). Here the whole path runs, so:
//!   - below H the arm refuses and the account is untouched (N5, H-1 leg);
//!   - at H a Stake debits the real account balance and records the deposit,
//!     a Sponsor burns from the deposit into the supply ledger, and a completed
//!     Withdraw credits the account back — all through the arm (N6 + burn/credit).
//!
//! ONE test: the feature schedule is installed process-wide (first-install-wins
//! `OnceLock`), so this file owns its process, the same discipline as
//! `passkey_activation_2169` and `feature_schedule_parity`.

use wrfcoin_shared_protocol::accounts::paymaster::PaymasterAction;
use wrfcoin_shared_protocol::blockchain::state::{State, StateError, StateTransition};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule,
};

const ACTIVATION_HEIGHT: u64 = 5;
const FUNDED: u128 = 10_000;
const STAKE: u128 = 1_000;
const DELAY: u64 = 2;
const SPONSOR_COST: u128 = 100;

fn apply(state: &mut State, action: PaymasterAction, block_height: u64) -> Result<(), StateError> {
    state.apply_transition(StateTransition::ApplyPaymasterAction { action, block_height })
}

#[test]
fn paymaster_arm_activates_debits_burns_and_credits_through_apply_transition() {
    let paymaster = Address([0x2b; 20]);
    let sender = Address([0x5e; 20]);

    // ── Schedule the feature at H, as the boot gate would. ──────────────────
    assert!(
        try_feature_schedule().is_none(),
        "this file owns its process; no schedule may pre-exist"
    );
    let raw = format!("0:v1,{ACTIVATION_HEIGHT}:{}", ConsensusFeature::Paymaster.name());
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);

    let mut state = State::new();
    state.create_account(paymaster, Balance::new(FUNDED));

    // ── H-1: below activation the arm refuses; the account is untouched. ─────
    let err = apply(
        &mut state,
        PaymasterAction::Stake { paymaster, amount: STAKE, unstake_delay_blocks: DELAY },
        ACTIVATION_HEIGHT - 1,
    )
    .unwrap_err();
    assert!(
        matches!(err, StateError::InvalidTransition),
        "below activation the arm must refuse via feature_active: {err:?}"
    );
    assert_eq!(
        state.get_account(&paymaster).unwrap().balance,
        Balance::new(FUNDED),
        "a refused stake moves no account balance"
    );
    assert_eq!(
        state.get_account(&paymaster).unwrap().spendable_balance,
        Balance::new(FUNDED),
        "a refused stake moves no spendable balance"
    );
    assert!(
        state.paymasters.is_empty(),
        "a refused stake records no deposit"
    );

    // ── H: Stake debits the real account balance and records the deposit. ───
    apply(
        &mut state,
        PaymasterAction::Stake { paymaster, amount: STAKE, unstake_delay_blocks: DELAY },
        ACTIVATION_HEIGHT,
    )
    .expect("stake applies at activation");
    assert_eq!(
        state.get_account(&paymaster).unwrap().balance,
        Balance::new(FUNDED - STAKE),
        "the arm debits the account balance by the staked amount"
    );
    assert_eq!(
        state.get_account(&paymaster).unwrap().spendable_balance,
        Balance::new(FUNDED - STAKE),
        "the arm debits spendable balance by the staked amount"
    );
    assert_eq!(
        state.paymasters.get(&paymaster).unwrap().deposit,
        STAKE,
        "the deposit is recorded"
    );

    // ── H: Sponsor burns the sponsored cost from the deposit into supply. ───
    let burn_before = state.supply_authorized_burning;
    apply(
        &mut state,
        PaymasterAction::SponsorOperation {
            paymaster,
            sender,
            actual_cost: SPONSOR_COST,
            succeeded: true,
        },
        ACTIVATION_HEIGHT,
    )
    .expect("sponsor applies at activation");
    assert_eq!(
        state.paymasters.get(&paymaster).unwrap().deposit,
        STAKE - SPONSOR_COST,
        "the sponsored cost is debited from the deposit"
    );
    assert_eq!(
        state.supply_authorized_burning,
        burn_before + SPONSOR_COST,
        "the arm burns the sponsored cost into the supply ledger"
    );

    // ── H..H+DELAY: request unstake, then withdraw credits the account. ─────
    apply(
        &mut state,
        PaymasterAction::RequestUnstake { paymaster },
        ACTIVATION_HEIGHT,
    )
    .expect("request unstake applies");
    apply(
        &mut state,
        PaymasterAction::WithdrawStake { paymaster },
        ACTIVATION_HEIGHT + DELAY,
    )
    .expect("withdraw applies once the delay has elapsed");
    assert_eq!(
        state.paymasters.get(&paymaster).unwrap().deposit,
        0,
        "the deposit is fully withdrawn"
    );
    assert_eq!(
        state.get_account(&paymaster).unwrap().spendable_balance,
        Balance::new(FUNDED - SPONSOR_COST),
        "the arm credits the remaining deposit back to the account"
    );
    assert_eq!(
        state.get_account(&paymaster).unwrap().balance,
        Balance::new(FUNDED - SPONSOR_COST),
        "balance is credited back through the arm"
    );
}
