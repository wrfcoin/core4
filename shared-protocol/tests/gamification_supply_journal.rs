use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::MAX_SUPPLY;
use wrfcoin_shared_protocol::invariants::balance::SharedSupplyInvariant;

fn payout(claim: &str, amount: u128) -> StateTransition {
    StateTransition::ApplyGamificationReward {
        address: Address([7; 20]),
        amount: Balance::new(amount),
        claim_id: claim.into(),
        current_block: 2,
    }
}

fn baselined() -> State {
    let mut state = State::new();
    state.apply_block_finalization(1);
    assert!(state.supply_violation.is_none());
    state.open_emission_block(2);
    state
}

#[test]
fn gamification_paid_mint_keeps_supply_balanced() {
    let mut state = baselined();
    let supervisor = SharedSupplyInvariant::new();
    state.supply_invariant = Some(supervisor.clone());
    state.apply_transition(payout("paid", 500)).unwrap();
    assert_eq!(state.get_account(&Address([7; 20])).unwrap().balance.0, 500);
    state.apply_block_finalization(2);
    assert!(
        state.supply_violation.is_none(),
        "authorized payout latched: {:?}",
        state.supply_violation
    );
    assert!(
        supervisor.violation().is_none(),
        "supervisor saw a false violation"
    );
    state.check_supply_conservation().unwrap();
    assert_eq!(state.supply_authorized_minting, 500);
}

#[test]
fn gamification_duplicate_does_not_journal_twice() {
    let mut state = baselined();
    state.apply_transition(payout("duplicate", 500)).unwrap();
    state.apply_transition(payout("duplicate", 999)).unwrap();
    assert_eq!(state.supply_authorized_minting, 500);
    state.apply_block_finalization(2);
    state.check_supply_conservation().unwrap();
}

#[test]
fn gamification_max_supply_deferral_does_not_journal() {
    let mut state = State::new();
    state.create_account(Address([8; 20]), Balance::new(MAX_SUPPLY));
    state.apply_block_finalization(1);
    state.open_emission_block(2);
    state.apply_transition(payout("cap", 1)).unwrap();
    assert_eq!(state.supply_authorized_minting, 0);
    assert!(!state.gamification_claims_seen.contains("cap"));
    state.apply_block_finalization(2);
    state.check_supply_conservation().unwrap();
}

#[test]
fn gamification_slice_refusal_does_not_journal_or_consume_claim() {
    let mut state = baselined();
    // Under MAX_SUPPLY, but greater than the entire per-block emission.
    state
        .apply_transition(payout("slice", MAX_SUPPLY / 2))
        .unwrap();
    assert_eq!(state.supply_authorized_minting, 0);
    assert!(!state.gamification_claims_seen.contains("slice"));
    assert_eq!(state.total_minted_supply(), 0);
    // Refusal leaves the slice and claim available for an affordable retry.
    state.apply_transition(payout("slice", 500)).unwrap();
    assert_eq!(state.supply_authorized_minting, 500);
    state.apply_block_finalization(2);
    state.check_supply_conservation().unwrap();
}

#[test]
fn gamification_journal_is_root_neutral() {
    let mut state = baselined();
    state.apply_transition(payout("root", 500)).unwrap();
    let material = state.state_root_material();
    let root = state.state_root;
    // Model the old implementation's post-payout journal; consensus stores
    // are identical. Journal changes must contribute no root bytes.
    state.supply_authorized_minting = 0;
    assert_eq!(state.state_root_material(), material);
    state.recalculate_state_root();
    assert_eq!(state.state_root, root);
}
