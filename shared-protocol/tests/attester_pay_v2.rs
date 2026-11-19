//! `attester_pay_v2`, scheduled at height `H` in this binary's schedule.
//!
//! Part 1, a correct reject vote is paid: the activation boundary, the split
//! across approved and rejected decisions in one block, approve wins where
//! both outcomes hold, stragglers, flipped votes, the threshold for several
//! set sizes, and a set change between the crossing and the paying block.
//!
//! Part 2, pay goes to the attester's bond payout account: binding through
//! the registration, the fallback to the identity's own address, a bond
//! update, exit and withdrawal, the refusals, and the signature over a
//! registration with and without `payout_bond`.
//!
//! Own test binary: the feature schedule is process-wide and the first
//! install wins, so every test goes through `install()`.

#[path = "common/node_bond_fixture.rs"]
mod helper;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, Once};

use helper::{A, address, advance, apply, auth, pk, resign, signature};
use wrfcoin_shared_protocol::blockchain::state::{
    ATTESTER_PAYOUT_BONDS_ROOT_TAG, SP_ATTESTER_PAYOUT_BOND_REFUSED, State, StateTransition,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    OracleAttestationPayload, ORACLE_REGISTRATION_PAYOUT_BOND_DOMAIN, Transaction,
    oracle_registration_authorization_message_with_domain,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::node_bond_registry::*;
use wrfcoin_shared_protocol::consensus::node_share_contracts::*;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::EmissionRole;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, feature_active, install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::oracle::attestation::{
    AttesterAllocation, AttesterSettlementRecord, reject_threshold,
};

/// Activation height of `attester_pay_v2` in this binary.
const H: u64 = 20;
const T0: i64 = 1_700_000_000;
const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
const GOVERNANCE_SEED: u8 = 0x77;
/// Oracle identity seeds start here, clear of every key the bond fixture uses.
const ORACLE_SEED: u8 = 0x30;

static INSTALL: Once = Once::new();
/// Makes every vote's nonce unique, so a repeated vote is a new transaction.
static NONCE: AtomicU64 = AtomicU64::new(0);
static SERIAL: Mutex<()> = Mutex::new(());

/// Install this binary's schedule (before the bond fixture can install its
/// own: the first install wins) and take the file-wide lock.
fn install() -> MutexGuard<'static, ()> {
    INSTALL.call_once(|| {
        install_feature_schedule(
            FeatureSchedule::from_entries(vec![
                ("node_share_settlement".into(), A),
                ("account_authorization_dispatch".into(), A),
                ("attester_pay_v2".into(), H),
            ])
            .unwrap(),
        );
        std::env::set_var(GOVERNANCE_ENV, PublicKey::from_private_key(&key(GOVERNANCE_SEED)).to_hex());
    });
    assert!(feature_active(ConsensusFeature::AttesterPayV2, H));
    assert!(!feature_active(ConsensusFeature::AttesterPayV2, H - 1));
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}

/// Oracle `n` (1-based): its signing key.
fn oracle_key(n: u8) -> PrivateKey {
    key(ORACLE_SEED + n)
}

/// Oracle `n`: its identity string (the hex public key).
fn oracle(n: u8) -> String {
    PublicKey::from_private_key(&oracle_key(n)).to_hex()
}

/// Oracle `n`: the address derived from its identity (where unbound pay goes).
fn own_address(n: u8) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(&oracle_key(n)))
}

/// The identities of `members`, sorted as the chain sorts them.
fn oracles(members: &[u8]) -> Vec<String> {
    let mut ids: Vec<String> = members.iter().map(|n| oracle(*n)).collect();
    ids.sort();
    ids
}

fn set_config(state: &mut State, members: &[u8], quorum: usize) {
    state
        .apply_transition(StateTransition::SetOracleConfig { oracle_set: oracles(members), oracle_quorum: quorum })
        .expect("oracle configuration applies");
}

/// A plain state with oracles `1..=n` and the given quorum.
fn plain(n: u8, quorum: usize) -> State {
    let mut state = State::new();
    set_config(&mut state, &(1..=n).collect::<Vec<_>>(), quorum);
    state
}

fn vote_at(state: &mut State, height: u64, decision: &str, n: u8, approved: bool) -> Result<(), String> {
    let mut payload = OracleAttestationPayload {
        commit_id: decision.to_string(),
        oracle_id: oracle(n),
        approved,
        signature: None,
        confidence: Some(1.0),
        timestamp: T0,
        nonce: Some(format!("nonce-{decision}-{n}-{approved}-{height}-{}", NONCE.fetch_add(1, Ordering::Relaxed))),
    };
    payload.signature = Some(oracle_key(n).sign(payload.signing_message().as_bytes()).to_string());
    state
        .apply_transition(StateTransition::RecordOracleAttestation { payload, current_block: height })
        .map_err(|e| e.to_string())
}

fn votes(state: &mut State, height: u64, decision: &str, voters: &[u8], approved: bool) {
    for n in voters {
        vote_at(state, height, decision, *n, approved).expect("vote applies");
    }
}

fn due(state: &State, decision: &str) -> Option<(u64, Vec<String>)> {
    state.attester_settlement_due.get(decision).map(|debt| (debt.crossing_height, debt.eligible.clone()))
}

/// Settle the whole outstanding debt at `height` as a producer stages it and
/// every applier checks it. Returns the records and the attester slice.
fn settle(state: &mut State, height: u64) -> (Vec<AttesterSettlementRecord>, u128) {
    state.open_emission_block(height);
    let outstanding: Vec<String> = state.attester_settlement_due.keys().cloned().collect();
    let slice = state.emission_ledger.remaining(EmissionRole::Attester);
    assert!(slice > 0, "the attester slice is funded at height {height}");
    let records = state.derive_attester_settlements(&outstanding, height, slice).expect("the debt is settleable");
    state
        .apply_transition(StateTransition::ApplyAttesterSettlements { records: records.clone() })
        .expect("settlement applies");
    (records, slice)
}

fn balance(state: &State, account: &Address) -> u128 {
    state.get_account(account).map_or(0, |a| a.balance.0)
}

fn paid(record: &AttesterSettlementRecord) -> Vec<&str> {
    record.allocations.iter().map(|a| a.attester_id.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Part 1: a correct reject vote is paid
// ---------------------------------------------------------------------------

#[test]
fn the_reject_threshold_opens_a_debt_only_from_the_activation() {
    let _guard = install();

    // Reached one block below the activation: no debt, then or ever.
    let mut below = plain(4, 2);
    votes(&mut below, H - 1, "d", &[1, 2, 3], false);
    assert!(below.attester_settlement_due.is_empty(), "no debt below the activation");
    let root_below = below.state_root;
    assert!(below.derive_attester_settlements(&["d".to_string()], H - 1, 1_000).is_err());
    // A fourth reject vote, a repeated vote and an approve vote at or above
    // the activation do not reopen a decision that was already past it.
    votes(&mut below, H, "d", &[4], false);
    vote_at(&mut below, H, "d", 1, false).expect("a repeated vote is refused without an error");
    assert!(below.attester_settlement_due.is_empty(), "a decision rejected before the activation is never owed");
    assert_ne!(below.state_root, root_below, "the votes themselves are recorded");
    assert!(below.derive_attester_settlements(&["d".to_string()], H, 1_000).is_err(), "nothing to settle without a debt");

    // Reached AT the activation: owed to the three reject voters.
    let mut at = plain(4, 2);
    votes(&mut at, H, "d", &[1, 2], false);
    assert!(at.attester_settlement_due.is_empty(), "two of three: below the threshold");
    votes(&mut at, H, "d", &[3], false);
    assert_eq!(due(&at, "d"), Some((H, oracles(&[1, 2, 3]))));

    // Reject votes that arrived before the activation, the last one AT it:
    // the debt opens at H and everyone present at the end of that block is
    // eligible, whenever their vote arrived.
    let mut across = plain(4, 2);
    votes(&mut across, H - 2, "d", &[1], false);
    votes(&mut across, H - 1, "d", &[2], false);
    assert!(across.attester_settlement_due.is_empty());
    let before = across.state_root_material();
    votes(&mut across, H, "d", &[3], false);
    assert_eq!(due(&across, "d"), Some((H, oracles(&[1, 2, 3]))));
    assert_ne!(across.state_root_material(), before);
    let (records, slice) = settle(&mut across, H);
    assert_eq!(records.len(), 1);
    assert_eq!(paid(&records[0]), oracles(&[1, 2, 3]).iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(records[0].total_amount(), slice, "the whole attester slice, nothing more");
    assert!(across.attester_settlement_due.is_empty());
    assert!(across.attester_settlements.contains_key("d"), "terminal marker");
    // Settled is final: a later vote does not reopen it.
    votes(&mut across, H + 1, "d", &[4], false);
    assert!(across.attester_settlement_due.is_empty());

    // The same three votes one block lower in the other state stay unpaid:
    // identical votes, different side of the boundary.
    let mut control = plain(4, 2);
    votes(&mut control, H - 3, "d", &[1], false);
    votes(&mut control, H - 2, "d", &[2], false);
    votes(&mut control, H - 1, "d", &[3], false);
    assert!(control.attester_settlement_due.is_empty());
}

#[test]
fn approve_pay_is_unchanged_across_the_activation() {
    let _guard = install();
    for height in [H - 1, H, H + 1] {
        let mut state = plain(4, 2);
        votes(&mut state, height, "d", &[1], true);
        assert!(state.attester_settlement_due.is_empty());
        votes(&mut state, height, "d", &[2], true);
        assert_eq!(due(&state, "d"), Some((height, oracles(&[1, 2]))), "height {height}");
        // A third approval in the crossing block joins; one a block later does not.
        votes(&mut state, height, "d", &[3], true);
        assert_eq!(due(&state, "d"), Some((height, oracles(&[1, 2, 3]))));
        votes(&mut state, height + 1, "d", &[4], true);
        assert_eq!(due(&state, "d"), Some((height, oracles(&[1, 2, 3]))));
    }
}

#[test]
fn approved_and_rejected_decisions_in_one_block_share_the_slice_in_remainder_order() {
    let _guard = install();
    let mut state = plain(4, 2);
    // "b-reject" sorts after "a-approve": the remainder goes to the approved
    // decision's attesters first, then the rejected one's, in identity order.
    votes(&mut state, H, "a-approve", &[1, 2], true);
    votes(&mut state, H, "b-reject", &[2, 3, 4], false);
    assert_eq!(due(&state, "a-approve"), Some((H, oracles(&[1, 2]))));
    assert_eq!(due(&state, "b-reject"), Some((H, oracles(&[2, 3, 4]))));
    let before: Vec<u128> = (1..=4).map(|n| balance(&state, &own_address(n))).collect();
    let (records, slice) = settle(&mut state, H);
    assert_eq!(records.iter().map(|r| r.decision_id.as_str()).collect::<Vec<_>>(), ["a-approve", "b-reject"]);

    // Independent arithmetic: floor(slice / 5) each, remainder one unit at a
    // time in ascending (decision id, attester id) order.
    let per = slice / 5;
    let mut remainder = slice % 5;
    let mut expected: Vec<AttesterSettlementRecord> = Vec::new();
    for (decision, voters) in [("a-approve", vec![1u8, 2]), ("b-reject", vec![2, 3, 4])] {
        let mut ids = oracles(&voters);
        ids.sort();
        let allocations = ids
            .into_iter()
            .map(|attester_id| {
                let extra = u128::from(remainder > 0);
                remainder -= extra;
                AttesterAllocation { attester_id, amount: per + extra }
            })
            .collect();
        expected.push(AttesterSettlementRecord::new(decision.to_string(), H, allocations));
    }
    assert_eq!(records, expected);
    assert_eq!(records.iter().map(AttesterSettlementRecord::total_amount).sum::<u128>(), slice);

    // Paid to each identity's own address: no binding exists.
    for n in 1..=4u8 {
        let earned: u128 = records
            .iter()
            .flat_map(|r| &r.allocations)
            .filter(|a| a.attester_id == oracle(n))
            .map(|a| a.amount)
            .sum();
        assert_eq!(balance(&state, &own_address(n)) - before[(n - 1) as usize], earned, "oracle {n}");
    }
    assert!(balance(&state, &own_address(1)) > 0 && balance(&state, &own_address(4)) > 0);
    // Record shape is the one every reader already parses.
    let json = serde_json::to_value(&records[1]).unwrap();
    let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["allocations", "decision_id", "finalization_height"]);

    // A tampered claim is refused: an approver named on the rejected decision.
    let mut again = plain(4, 2);
    votes(&mut again, H, "b-reject", &[2, 3, 4], false);
    votes(&mut again, H, "b-reject", &[1], true);
    again.open_emission_block(H);
    let slice = again.emission_ledger.remaining(EmissionRole::Attester);
    let honest = again.derive_attester_settlements(&["b-reject".to_string()], H, slice).unwrap();
    assert_eq!(paid(&honest[0]).len(), 3, "the approver on a rejected decision is not paid");
    let mut forged = honest.clone();
    forged[0].allocations[0].attester_id = oracle(1);
    forged[0] = AttesterSettlementRecord::new("b-reject".into(), H, forged[0].allocations.clone());
    let root = again.state_root;
    assert!(again.apply_transition(StateTransition::ApplyAttesterSettlements { records: forged }).is_err());
    assert_eq!(again.state_root, root);
    again.apply_transition(StateTransition::ApplyAttesterSettlements { records: honest }).expect("the derived claim applies");
}

#[test]
fn approve_wins_when_both_outcomes_hold_at_the_end_of_the_crossing_block() {
    let _guard = install();
    let mut state = plain(4, 2);
    votes(&mut state, H, "d", &[1, 2, 3], false);
    assert_eq!(due(&state, "d"), Some((H, oracles(&[1, 2, 3]))), "rejected so far");
    // The set changes inside the block (3 leaves, 5 joins): n = 4, quorum 2,
    // reject threshold 3. The three reject votes still stand.
    set_config(&mut state, &[1, 2, 4, 5], 2);
    votes(&mut state, H, "d", &[4], true);
    assert_eq!(due(&state, "d"), Some((H, oracles(&[1, 2, 3]))), "one approval: still rejected");
    votes(&mut state, H, "d", &[5], true);
    let set = &state.attestations["d"];
    assert!(set.has_quorum(2) && set.has_reject_threshold(4, 2), "both outcomes hold");
    let mut approvers = oracles(&[4, 5]);
    approvers.sort();
    assert_eq!(due(&state, "d"), Some((H, approvers.clone())), "approve wins");
    let (records, _) = settle(&mut state, H);
    assert_eq!(paid(&records[0]), approvers.iter().map(String::as_str).collect::<Vec<_>>());

    // Approval first, rejections after, same block: approval is never displaced.
    let mut state = plain(4, 2);
    votes(&mut state, H, "d", &[1, 2], true);
    votes(&mut state, H, "d", &[3, 4], false);
    assert_eq!(due(&state, "d"), Some((H, oracles(&[1, 2]))));

    // A reject debt from an EARLIER block is not rewritten by a later approval.
    let mut state = plain(4, 2);
    votes(&mut state, H, "d", &[1, 2, 3], false);
    set_config(&mut state, &[1, 2, 4, 5], 2);
    votes(&mut state, H + 1, "d", &[4, 5], true);
    assert_eq!(due(&state, "d"), Some((H, oracles(&[1, 2, 3]))), "frozen at the crossing block");
}

#[test]
fn a_straggler_is_unpaid_and_a_flipped_vote_is_still_refused() {
    let _guard = install();
    let mut state = plain(4, 2);
    votes(&mut state, H, "d", &[1, 2, 3], false);
    // The crossing block does not settle; the fourth reject vote lands later.
    votes(&mut state, H + 1, "d", &[4], false);
    assert_eq!(due(&state, "d"), Some((H, oracles(&[1, 2, 3]))), "the straggler does not join");
    let straggler_before = balance(&state, &own_address(4));
    let (records, slice) = settle(&mut state, H + 1);
    assert_eq!(paid(&records[0]).len(), 3);
    assert_eq!(records[0].finalization_height, H + 1);
    assert_eq!(records[0].total_amount(), slice);
    assert_eq!(balance(&state, &own_address(4)), straggler_before, "the straggler is unpaid");

    // A flipped vote is refused under the new rule exactly as before, in
    // both directions, and changes neither the vote nor the debt.
    let mut state = plain(4, 2);
    votes(&mut state, H, "d", &[1, 2], false);
    assert!(vote_at(&mut state, H, "d", 1, true).is_err(), "reject then approve");
    assert!(!state.attestations["d"].by_oracle[&oracle(1)].approved);
    assert!(state.attester_settlement_due.is_empty());
    votes(&mut state, H, "e", &[3], true);
    assert!(vote_at(&mut state, H, "e", 3, false).is_err(), "approve then reject");
    assert_eq!(state.attestations["e"].rejections(), 0);
    // A repeated reject vote does not count twice toward the threshold.
    vote_at(&mut state, H, "d", 2, false).expect("refused without an error");
    assert_eq!(state.attestations["d"].rejections(), 2);
    assert!(state.attester_settlement_due.is_empty());
}

#[test]
fn the_threshold_holds_for_each_set_size_and_quorum() {
    let _guard = install();
    for (n, quorum, threshold) in [(4u8, 2usize, 3u8), (3, 2, 2), (5, 3, 3), (7, 3, 5)] {
        assert_eq!(reject_threshold(n as usize, quorum), threshold as usize);
        let mut state = plain(n, quorum);
        let short: Vec<u8> = (1..threshold).collect();
        votes(&mut state, H, "d", &short, false);
        assert!(state.attester_settlement_due.is_empty(), "{n}/{quorum}: one short of the threshold");
        assert!(state.derive_attester_settlements(&["d".to_string()], H, 1_000).is_err());
        votes(&mut state, H, "d", &[threshold], false);
        let mut expected = oracles(&(1..=threshold).collect::<Vec<_>>());
        expected.sort();
        assert_eq!(due(&state, "d"), Some((H, expected.clone())), "{n}/{quorum}");
        let (records, slice) = settle(&mut state, H);
        assert_eq!(paid(&records[0]), expected.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(records[0].total_amount(), slice);
        // Approval could no longer be reached: fewer than `quorum` identities are left.
        assert!((n - threshold) < quorum as u8);
    }
}

#[test]
fn a_set_change_between_the_crossing_and_the_paying_block() {
    let _guard = install();
    // An eligible identity leaves the set before the debt is paid: the
    // threshold still holds (n = 4), and the frozen identities are paid.
    let mut state = plain(4, 2);
    votes(&mut state, H, "d", &[1, 2, 3], false);
    set_config(&mut state, &[1, 2, 4, 5], 2);
    assert_eq!(due(&state, "d"), Some((H, oracles(&[1, 2, 3]))));
    let before = balance(&state, &own_address(3));
    let (records, _) = settle(&mut state, H + 1);
    assert_eq!(paid(&records[0]).len(), 3);
    assert!(balance(&state, &own_address(3)) > before, "the departed identity is paid what it was owed");

    // The set grows before the debt is paid (n = 5: threshold 4). The three
    // reject votes no longer hold the live threshold, but the debt is the
    // proof of the crossing: it is still paid, to the frozen three, and an
    // approve decision owed in the same block is paid with it.
    let mut state = plain(4, 2);
    votes(&mut state, H, "d", &[1, 2, 3], false);
    votes(&mut state, H, "e", &[1, 2], true);
    set_config(&mut state, &[1, 2, 3, 4, 5], 2);
    assert_eq!(due(&state, "d"), Some((H, oracles(&[1, 2, 3]))), "still owed");
    assert_eq!(due(&state, "e"), Some((H, oracles(&[1, 2]))), "still owed");
    let (records, _) = settle(&mut state, H + 1);
    assert_eq!(records.len(), 2, "one unpayable debt must not hold back the others");
    assert_eq!(paid(&records[0]).len(), 3);
    assert_eq!(paid(&records[1]).len(), 2);
    assert_eq!(due(&state, "d"), None);
    assert_eq!(due(&state, "e"), None);

    // Quorum raised before an approve debt is paid: also still paid.
    let mut state = plain(4, 2);
    votes(&mut state, H, "e", &[1, 2], true);
    set_config(&mut state, &[1, 2, 3, 4], 3);
    let (records, _) = settle(&mut state, H + 1);
    assert_eq!(paid(&records[0]).len(), 2);

    // Below the activation nothing changes: an approve debt whose quorum was
    // raised after the crossing is still refused at H - 1, exactly as before,
    // and the same state pays it at H.
    let mut state = plain(4, 2);
    votes(&mut state, H - 1, "e", &[1, 2], true);
    set_config(&mut state, &[1, 2, 3, 4], 3);
    state.open_emission_block(H - 1);
    let slice = state.emission_ledger.remaining(EmissionRole::Attester);
    let refused = state.derive_attester_settlements(&["e".to_string()], H - 1, slice).unwrap_err();
    assert!(refused.to_string().contains("no oracle quorum"), "{refused}");
    assert_eq!(due(&state, "e"), Some((H - 1, oracles(&[1, 2]))), "still owed below the activation");
    let (records, _) = settle(&mut state, H);
    assert_eq!(paid(&records[0]).len(), 2);

    // A decision with no debt entry is never payable, whatever is named.
    let mut state = plain(4, 2);
    votes(&mut state, H, "d", &[1, 2], false);
    state.open_emission_block(H + 1);
    let slice = state.emission_ledger.remaining(EmissionRole::Attester);
    let refused = state.derive_attester_settlements(&["d".to_string()], H + 1, slice).unwrap_err();
    assert!(refused.to_string().contains("no oracle quorum"), "{refused}");
}

// ---------------------------------------------------------------------------
// Part 2: pay goes to the attester's bond payout account
// ---------------------------------------------------------------------------

fn bond_hex(id: u8) -> String {
    hex::encode([id; 32])
}

/// The bond fixture's funded state with bond 1 (owner and payout account
/// `address(10)`) and bond 2 (`address(20)`) deposited, four oracles, quorum 2.
fn bonded() -> State {
    let mut state = helper::state();
    apply(&mut state, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).expect("bond 1");
    apply(&mut state, auth(20, 2, 0, NodeBondAction::Deposit, 1), A).expect("bond 2");
    set_config(&mut state, &[1, 2, 3, 4], 2);
    state
}

/// A governance-signed registration for oracle `n` over `members`, naming
/// `payout_bond`. Each call registers a fresh signing-key epoch.
fn registration(members: &[u8], n: u8, epoch: u64, payout_bond: Option<&str>) -> Transaction {
    let identity = ChainIdentity::active();
    Transaction::new_oracle_registration_with_payout_bond_and_domain(
        &identity.chain_id,
        &identity.network_id,
        oracles(members),
        2,
        &oracle(n),
        &PublicKey::from_private_key(&key(0x80 + n * 8 + epoch as u8)).to_hex(),
        epoch,
        T0 + 1_000,
        payout_bond,
        &key(GOVERNANCE_SEED),
    )
}

fn register(state: &mut State, height: u64, tx: &Transaction) -> Result<(), String> {
    let step = tx.oracle_registration_step(height).ok_or("no transition")?;
    state.apply_transition(step).map_err(|e| e.to_string())
}

#[test]
fn a_bound_attester_is_paid_to_the_bond_payout_account_and_an_unbound_one_to_its_own_address() {
    let _guard = install();
    let mut state = bonded();
    register(&mut state, H, &registration(&[1, 2, 3, 4], 1, 1, Some(&bond_hex(1)))).expect("binds oracle 1");
    assert_eq!(state.attester_payout_bonds.get(&oracle(1)), Some(&[1u8; 32]));
    assert_eq!(state.attester_bond_payout_account(&oracle(1)), Some(address(10)));
    assert_eq!(state.attester_settlement_payee(&oracle(1), H), Some(address(10)));
    assert_eq!(state.attester_settlement_payee(&oracle(1), H - 1), Some(own_address(1)), "below the activation");
    assert_eq!(state.attester_settlement_payee(&oracle(2), H), Some(own_address(2)), "unbound");
    let material = state.state_root_material();
    assert!(material.windows(ATTESTER_PAYOUT_BONDS_ROOT_TAG.len()).any(|w| w == ATTESTER_PAYOUT_BONDS_ROOT_TAG));
    assert!(is_protocol_credited_address(&state, &address(10)));

    // A rejected decision: oracle 1 (bound) and 2, 3 (unbound) are owed.
    votes(&mut state, H, "d", &[1, 2, 3], false);
    let payout_before = balance(&state, &address(10));
    let (records, _) = settle(&mut state, H);
    let amount = |n: u8| records[0].allocations.iter().find(|a| a.attester_id == oracle(n)).unwrap().amount;
    assert!(amount(1) > 0);
    assert_eq!(balance(&state, &address(10)) - payout_before, amount(1), "paid to the bond payout account");
    assert_eq!(balance(&state, &own_address(1)), 0, "not to the identity's own address");
    assert_eq!(balance(&state, &own_address(2)), amount(2));
    assert_eq!(balance(&state, &own_address(3)), amount(3));
    // The record names the attester, never an address.
    assert_eq!(serde_json::to_value(&records[0].allocations[0]).unwrap().as_object().unwrap().len(), 2);

    // An approved decision pays the bound attester the same way.
    votes(&mut state, H + 1, "e", &[1, 4], true);
    let payout_before = balance(&state, &address(10));
    let (records, _) = settle(&mut state, H + 1);
    let to_one = records[0].allocations.iter().find(|a| a.attester_id == oracle(1)).unwrap().amount;
    assert_eq!(balance(&state, &address(10)) - payout_before, to_one);
    assert_eq!(balance(&state, &own_address(1)), 0);
}

#[test]
fn a_registration_naming_a_bond_is_refused_unless_the_bond_is_registered_and_free() {
    let _guard = install();
    let mut state = bonded();
    register(&mut state, H, &registration(&[1, 2, 3, 4], 1, 1, Some(&bond_hex(1)))).expect("binds oracle 1");

    let refused = |state: &mut State, tx: &Transaction| {
        let root = state.state_root;
        let material = state.state_root_material();
        let bytes = bincode::serialize(&*state).unwrap();
        let error = register(state, H, tx).expect_err("refused");
        assert!(error.contains(SP_ATTESTER_PAYOUT_BOND_REFUSED), "{error}");
        assert_eq!(state.state_root, root);
        assert!(state.state_root_material() == material, "a refusal writes nothing");
        assert!(bincode::serialize(&*state).unwrap() == bytes, "a refusal writes nothing");
    };
    // One seat per bond: a second identity naming bond 1.
    refused(&mut state, &registration(&[1, 2, 3, 4], 2, 1, Some(&bond_hex(1))));
    // A bond that is not registered.
    refused(&mut state, &registration(&[1, 2, 3, 4], 2, 1, Some(&bond_hex(9))));
    // Not 32 bytes of hex.
    for bad in ["", "zz", &"ab".repeat(31), &"ab".repeat(33), &format!("0x{}", "01".repeat(31)), &"g1".repeat(32)] {
        refused(&mut state, &registration(&[1, 2, 3, 4], 2, 1, Some(bad)));
    }
    assert_eq!(state.attester_payout_bonds.len(), 1);
    // Upper-case hex names the same bond; a free bond is accepted.
    register(&mut state, H, &registration(&[1, 2, 3, 4], 2, 1, Some(&bond_hex(2).to_uppercase()))).expect("bond 2 is free");
    assert_eq!(state.attester_payout_bonds.get(&oracle(2)), Some(&[2u8; 32]));
    // The same identity naming its own bond again is not "another identity".
    register(&mut state, H, &registration(&[1, 2, 3, 4], 1, 2, Some(&bond_hex(1)))).expect("re-registration keeps the bond");

    // A registration that fails for another reason leaves the bindings alone:
    // the identity is not in the set it installs.
    let root = state.state_root;
    assert!(register(&mut state, H, &registration(&[1, 2, 3], 4, 1, None)).is_err());
    assert_eq!(state.state_root, root);
    assert_eq!(state.attester_payout_bonds.len(), 2);

    // The new transition is refused below the activation.
    let below = StateTransition::ApplyOracleRegistrationWithPayoutBond {
        oracle_set: oracles(&[1, 2, 3, 4]),
        oracle_quorum: 2,
        identity: oracle(3),
        public_key: oracle(3),
        epoch: 5,
        activated_at: T0,
        payout_bond: None,
        block_height: H - 1,
    };
    assert!(state.apply_transition(below).is_err());
}

#[test]
fn a_bond_without_its_full_principal_is_not_paid_and_cannot_be_named() {
    let _guard = install();
    let mut state = bonded();
    register(&mut state, H, &registration(&[1, 2, 3, 4], 1, 1, Some(&bond_hex(1)))).expect("binds oracle 1");
    assert_eq!(state.attester_settlement_payee(&oracle(1), H), Some(address(10)));

    // Bond 1 loses its principal (a forfeited bond keeps its registration
    // entry). The binding stays in the map, but pay falls back to the
    // identity's own address, and no registration may name the bond.
    state
        .node_bond_registry
        .as_mut()
        .expect("registry")
        .registrations
        .get_mut(&[1u8; 32])
        .expect("bond 1")
        .principal_units = 0;
    assert_eq!(state.attester_bond_payout_account(&oracle(1)), None);
    assert_eq!(state.attester_settlement_payee(&oracle(1), H + 1), Some(own_address(1)));
    votes(&mut state, H + 1, "d", &[1, 2, 3], false);
    let payout_before = balance(&state, &address(10));
    let (records, _) = settle(&mut state, H + 1);
    let to_one = records[0].allocations.iter().find(|a| a.attester_id == oracle(1)).unwrap().amount;
    assert!(to_one > 0);
    assert_eq!(balance(&state, &own_address(1)), to_one, "paid to the identity's own address");
    assert_eq!(balance(&state, &address(10)), payout_before, "not to the bond payout account");
    let refused = register(&mut state, H + 2, &registration(&[1, 2, 3, 4], 2, 1, Some(&bond_hex(1)))).unwrap_err();
    assert!(refused.contains(SP_ATTESTER_PAYOUT_BOND_REFUSED), "{refused}");
    // A bond in good standing is still accepted.
    register(&mut state, H + 2, &registration(&[1, 2, 3, 4], 2, 1, Some(&bond_hex(2)))).expect("bond 2");
}

#[test]
fn a_binding_is_absolute_per_identity_and_leaves_with_the_identity() {
    let _guard = install();
    let mut state = bonded();
    register(&mut state, H, &registration(&[1, 2, 3, 4], 1, 1, Some(&bond_hex(1)))).unwrap();
    register(&mut state, H, &registration(&[1, 2, 3, 4], 2, 1, Some(&bond_hex(2)))).unwrap();
    assert_eq!(state.attester_payout_bonds.len(), 2);

    // A registration for oracle 1 that names no bond removes its binding and
    // touches no other identity's.
    register(&mut state, H + 1, &registration(&[1, 2, 3, 4], 1, 2, None)).unwrap();
    assert_eq!(state.attester_payout_bonds.keys().collect::<Vec<_>>(), [&oracle(2)]);
    assert_eq!(state.attester_settlement_payee(&oracle(1), H + 1), Some(own_address(1)));
    // Bond 1 is free again: oracle 3 may take it.
    register(&mut state, H + 1, &registration(&[1, 2, 3, 4], 3, 1, Some(&bond_hex(1)))).unwrap();

    // A registration whose set drops oracle 2 drops oracle 2's binding in the
    // same apply, and oracle 4 may name bond 2 in that very registration.
    register(&mut state, H + 2, &registration(&[1, 3, 4, 5], 4, 1, Some(&bond_hex(2)))).unwrap();
    assert!(!state.attester_payout_bonds.contains_key(&oracle(2)));
    assert_eq!(state.attester_payout_bonds.get(&oracle(4)), Some(&[2u8; 32]));
    assert_eq!(state.attester_payout_bonds.get(&oracle(3)), Some(&[1u8; 32]));

    // Every other way the set is replaced drops the bindings of the removed
    // identities too, so the map never names an identity outside the set.
    set_config(&mut state, &[1, 4, 5, 6], 2);
    assert_eq!(state.attester_payout_bonds.keys().collect::<Vec<_>>(), [&oracle(4)]);
    set_config(&mut state, &[1, 5, 6], 2);
    assert!(state.attester_payout_bonds.is_empty());
    let material = state.state_root_material();
    assert!(
        !material.windows(ATTESTER_PAYOUT_BONDS_ROOT_TAG.len()).any(|w| w == ATTESTER_PAYOUT_BONDS_ROOT_TAG),
        "an empty map leaves no section in the root"
    );
}

#[test]
fn a_bond_update_changes_the_payee_when_it_takes_effect_and_a_withdrawn_bond_falls_back() {
    let _guard = install();
    let mut state = bonded();
    register(&mut state, H, &registration(&[1, 2, 3, 4], 1, 1, Some(&bond_hex(1)))).unwrap();
    assert_eq!(state.attester_settlement_payee(&oracle(1), H), Some(address(10)));

    // Rotate bond 1's payout account to address(30); pending until the next epoch.
    let boundary = A + NODE_EPOCH_BLOCKS;
    advance(&mut state, boundary);
    let mut update = auth(10, 1, 1, NodeBondAction::Update, 2);
    update.signed.authorization.payout_account = address(30);
    resign(&mut update, 10);
    update.payout_key = Some(pk(30));
    update.payout_signature = signature(&update.signed.authorization, 30);
    apply(&mut state, update, boundary).expect("update");
    assert!(state.node_bond_registry.as_ref().unwrap().registrations[&[1; 32]].pending_binding.is_some());
    assert_eq!(state.attester_settlement_payee(&oracle(1), boundary), Some(address(10)), "a pending binding does not pay");
    votes(&mut state, boundary, "d1", &[1, 2, 3], false);
    let (records, _) = settle(&mut state, boundary);
    let to_one = records[0].allocations.iter().find(|a| a.attester_id == oracle(1)).unwrap().amount;
    assert_eq!(balance(&state, &address(30)), 0);
    assert!(balance(&state, &address(10)) >= to_one);

    // The update takes effect at the next epoch start.
    let next = A + 2 * NODE_EPOCH_BLOCKS;
    advance(&mut state, next);
    assert_eq!(state.attester_settlement_payee(&oracle(1), next), Some(address(30)));
    votes(&mut state, next, "d2", &[1, 2, 3], false);
    let old_before = balance(&state, &address(10));
    let (records, _) = settle(&mut state, next);
    let to_one = records[0].allocations.iter().find(|a| a.attester_id == oracle(1)).unwrap().amount;
    assert_eq!(balance(&state, &address(30)), to_one, "the updated payout account is paid");
    assert_eq!(balance(&state, &address(10)), old_before);

    // Exit: the bond is still registered until it is withdrawn, so it still pays.
    let mut exit = auth(10, 1, 2, NodeBondAction::Exit, 3);
    exit.signed.authorization.payout_account = address(30);
    exit.signed.owner_signature = signature(&exit.signed.authorization, 10);
    apply(&mut state, exit, next).expect("exit");
    assert_eq!(state.attester_settlement_payee(&oracle(1), next), Some(address(30)));

    // Withdrawn: the registration is gone and pay falls back to the
    // identity's own address. The binding itself stays, harmless.
    let exit_height = A + 3 * NODE_EPOCH_BLOCKS;
    let boundary = BondExitBoundary::checked(A, exit_height, TESTNET_CHALLENGE_BLOCKS).unwrap();
    let h = boundary.earliest_withdrawal_height;
    advance(&mut state, h);
    let mut withdraw = auth(10, 1, 3, NodeBondAction::Withdraw, epoch_at(A, h).unwrap());
    withdraw.signed.authorization.payout_account = address(30);
    withdraw.signed.owner_signature = signature(&withdraw.signed.authorization, 10);
    apply(&mut state, withdraw, h).expect("withdraw");
    assert!(!state.node_bond_registry.as_ref().unwrap().registrations.contains_key(&[1; 32]));
    assert_eq!(state.attester_payout_bonds.get(&oracle(1)), Some(&[1u8; 32]));
    assert_eq!(state.attester_bond_payout_account(&oracle(1)), None);
    assert_eq!(state.attester_settlement_payee(&oracle(1), h), Some(own_address(1)));
    votes(&mut state, h, "d3", &[1, 2, 3], false);
    let rotated_before = balance(&state, &address(30));
    let (records, _) = settle(&mut state, h);
    let to_one = records[0].allocations.iter().find(|a| a.attester_id == oracle(1)).unwrap().amount;
    assert_eq!(balance(&state, &own_address(1)), to_one, "fallback to the identity's own address");
    assert_eq!(balance(&state, &address(30)), rotated_before);
    // A withdrawn bond can no longer be named.
    let error = register(&mut state, h, &registration(&[1, 2, 3, 4], 3, 1, Some(&bond_hex(1)))).unwrap_err();
    assert!(error.contains(SP_ATTESTER_PAYOUT_BOND_REFUSED), "{error}");
}

#[test]
fn the_governance_signature_covers_payout_bond_only_when_it_is_present() {
    let _guard = install();
    let identity = ChainIdentity::active();
    let without = registration(&[1, 2, 3, 4], 1, 1, None);
    let with = registration(&[1, 2, 3, 4], 1, 1, Some(&bond_hex(1)));

    // Without the member: the transaction, its bytes and its message are what
    // the earlier constructor builds.
    let earlier = Transaction::new_oracle_registration_with_domain(
        &identity.chain_id,
        &identity.network_id,
        oracles(&[1, 2, 3, 4]),
        2,
        &oracle(1),
        &PublicKey::from_private_key(&key(0x80 + 8 + 1)).to_hex(),
        1,
        T0 + 1_000,
        &key(GOVERNANCE_SEED),
    );
    assert_eq!(without.data, earlier.data);
    assert_eq!(without.inputs[0].txid, earlier.inputs[0].txid);
    assert!(!without.data.as_deref().unwrap().contains("payout_bond"));
    let payload = without.parse_oracle_registration().unwrap();
    let base = oracle_registration_authorization_message_with_domain(&identity.chain_id, &identity.network_id, &payload);
    assert!(!base.windows(ORACLE_REGISTRATION_PAYOUT_BOND_DOMAIN.len()).any(|w| w == ORACLE_REGISTRATION_PAYOUT_BOND_DOMAIN));
    for height in [0, H - 1, H, u64::MAX] {
        assert!(without.verify_oracle_registration_authorization_at(height), "height {height}");
        assert_eq!(without.parse_oracle_registration_at(height).unwrap().payout_bond, None);
    }
    assert!(without.verify_oracle_registration_authorization());

    // With the member: the message is the base message, then the tag, then
    // the length-prefixed value.
    let payload = with.parse_oracle_registration_at(H).unwrap();
    assert_eq!(payload.payout_bond.as_deref(), Some(bond_hex(1).as_str()));
    let message = oracle_registration_authorization_message_with_domain(&identity.chain_id, &identity.network_id, &payload);
    let mut expected = base.clone();
    expected.extend_from_slice(ORACLE_REGISTRATION_PAYOUT_BOND_DOMAIN);
    expected.extend_from_slice(&64u32.to_le_bytes());
    expected.extend_from_slice(bond_hex(1).as_bytes());
    assert_eq!(message, expected);
    assert!(with.verify_oracle_registration_authorization_at(H));
    assert!(matches!(
        with.oracle_registration_step(H),
        Some(StateTransition::ApplyOracleRegistrationWithPayoutBond { payout_bond: Some(bond), block_height: H, .. }) if bond == bond_hex(1)
    ));
    // Below the activation the member is not read, so the signature (which
    // covers it) does not verify there: the registration is valid only from H.
    assert_eq!(with.parse_oracle_registration_at(H - 1).unwrap().payout_bond, None);
    assert!(!with.verify_oracle_registration_authorization_at(H - 1));
    assert!(!with.verify_oracle_registration_authorization());
    assert!(with.oracle_registration_step(H - 1).is_none());

    // The member is signature-bound from the activation: changing it,
    // removing it or adding it breaks the authorization.
    let edit = |tx: &Transaction, from: &str, to: &str| {
        let mut edited = tx.clone();
        let data = edited.data.clone().unwrap();
        assert!(data.contains(from));
        edited.data = Some(data.replacen(from, to, 1));
        edited
    };
    let member = format!(",\"payout_bond\":\"{}\"", bond_hex(1));
    let redirected = edit(&with, &bond_hex(1), &bond_hex(2));
    assert!(!redirected.verify_oracle_registration_authorization_at(H));
    assert!(redirected.oracle_registration_step(H).is_none());
    let stripped = edit(&with, &member, "");
    assert!(!stripped.verify_oracle_registration_authorization_at(H));
    let added = edit(&without, "{", &format!("{{\"payout_bond\":\"{}\",", bond_hex(2)));
    assert!(!added.verify_oracle_registration_authorization_at(H), "a bond added after signing");
    assert!(added.oracle_registration_step(H).is_none());

    // Below the activation that added member is ignored exactly as a binary
    // without the field ignores it: the registration is the unedited one.
    assert!(added.verify_oracle_registration_authorization_at(H - 1));
    assert_eq!(
        format!("{:?}", added.oracle_registration_step(H - 1)),
        format!("{:?}", without.oracle_registration_step(H - 1))
    );
    assert!(matches!(added.oracle_registration_step(H - 1), Some(StateTransition::ApplyOracleRegistration { .. })));
    // Any JSON at all under that name, and the name twice, below the activation.
    for odd in ["7", "null", "[1,2]", "{\"a\":{}}", "\"zz\"", "true"] {
        let tx = edit(&without, "{", &format!("{{\"payout_bond\":{odd},\"payout_bond\":{odd},"));
        assert_eq!(tx.parse_oracle_registration(), without.parse_oracle_registration(), "{odd}");
        assert_eq!(tx.parse_oracle_registration_at(H - 1), without.parse_oracle_registration(), "{odd}");
        assert!(tx.verify_oracle_registration_authorization_at(H - 1), "{odd}");
    }
    // From the activation a malformed member makes the registration unparseable.
    for odd in ["7", "[1,2]", "{\"a\":{}}", "true"] {
        let tx = edit(&without, "{", &format!("{{\"payout_bond\":{odd},"));
        assert!(tx.parse_oracle_registration_at(H).is_none(), "{odd}");
        assert!(!tx.verify_oracle_registration_authorization_at(H), "{odd}");
        assert!(tx.oracle_registration_step(H).is_none(), "{odd}");
    }
    let twice = edit(&with, "{", &format!("{{\"payout_bond\":\"{}\",", bond_hex(1)));
    assert!(twice.parse_oracle_registration_at(H).is_none(), "the member twice");
    // An explicit null is "no bond" and keeps the plain signature valid.
    let null = edit(&without, "{", "{\"payout_bond\":null,");
    assert!(null.verify_oracle_registration_authorization_at(H));
    assert!(matches!(
        null.oracle_registration_step(H),
        Some(StateTransition::ApplyOracleRegistrationWithPayoutBond { payout_bond: None, .. })
    ));
}

#[test]
fn before_the_activation_a_registration_applies_exactly_as_one_without_the_member() {
    let _guard = install();
    let plain_tx = registration(&[1, 2, 3, 4], 1, 1, None);
    let mut with_member = plain_tx.clone();
    with_member.data = Some(
        plain_tx.data.clone().unwrap().replacen('{', &format!("{{\"payout_bond\":\"{}\",", bond_hex(1)), 1),
    );
    let mut a = bonded();
    let mut b = bonded();
    register(&mut a, H - 1, &plain_tx).expect("applies");
    register(&mut b, H - 1, &with_member).expect("applies with the member ignored");
    assert_eq!(a.state_root, b.state_root);
    assert!(a.state_root_material() == b.state_root_material());
    assert!(b.attester_payout_bonds.is_empty(), "no binding below the activation");
    // And pay below the activation goes to the identity's own address.
    votes(&mut b, H - 1, "d", &[1, 2], true);
    let (records, _) = settle(&mut b, H - 1);
    let to_one = records[0].allocations.iter().find(|x| x.attester_id == oracle(1)).unwrap().amount;
    assert_eq!(balance(&b, &own_address(1)), to_one);
}
