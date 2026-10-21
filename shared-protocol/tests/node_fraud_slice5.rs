#[path = "common/node_receipt_fixture.rs"]
mod f;
use wrfcoin_shared_protocol::{
    blockchain::{
        state::{State, StateTransition},
        state_snapshot::{export_state_entries, import_state_entries},
    },
    common::types::Balance,
    consensus::{
        node_bond_registry::{NODE_BOND_UNITS, epoch_close, node_bond_escrow_address},
        node_fraud::*,
        node_settlement::EpochStatus,
        node_share_contracts::*,
    },
};

fn report(claims: Vec<BondedServiceClaim>, nonce: u64) -> NodeFraudReport {
    let mut p = f::submission(claims, nonce);
    p.authorization.batch_hash = evidence_hash(&p).unwrap();
    p.owner_signature = f::sign(&p.authorization, 10);
    p
}
fn counterfeit(s: &State) -> BondedServiceClaim {
    let mut c = f::claim(s, 1);
    c.receipt.signature[0] ^= 1;
    f::resign_outer(&mut c, 11, 22);
    c
}
fn apply(
    s: &mut State,
    p: NodeFraudReport,
    height: u64,
) -> Result<(), wrfcoin_shared_protocol::blockchain::state::StateError> {
    s.apply_transition(StateTransition::ApplyNodeFraudReport {
        payload: p,
        block_height: height,
    })
}
fn advance(s: &mut State, height: u64) {
    let start = s.node_bond_registry.as_ref().unwrap().block_height;
    s.apply_block_finalization(start);
    f::bonds::advance(s, height);
    assert!(s.supply_violation.is_none(), "{:?}", s.supply_violation);
}
fn jailed(s: &State, id: u8) -> bool {
    s.node_fraud.as_ref().unwrap().jailed.contains(&[id; 32])
}

#[test]
fn forged_named_victim_and_missing_outer_attribution_are_atomic_refusals() {
    let mut s = f::state();
    let before = s.state_root_material();
    let mut c = counterfeit(&s);
    c.server_signature[0] ^= 1;
    assert!(apply(&mut s, report(vec![c], 1), f::H).is_err());
    assert_eq!(s.state_root_material(), before);
    let mut c = counterfeit(&s);
    c.server_signature.clear();
    assert!(apply(&mut s, report(vec![c], 1), f::H).is_err());
    assert_eq!(s.state_root_material(), before);
    assert!(!jailed(&s, 1));
    assert!(!jailed(&s, 2));
    let c = f::claim(&s, 1);
    assert!(
        apply(&mut s, report(vec![c], 1), f::H).is_err(),
        "honest work is not counterfeit"
    );
    assert_eq!(s.state_root_material(), before);
}

#[test]
fn authentic_counterfeit_debits_responsible_escrow_not_victim_and_duplicate_never_debits_twice() {
    let mut s = f::state();
    let initial = s.total_minted_supply();
    let minted = s.supply_authorized_minting;
    let burned = s.supply_authorized_burning;
    let victim = s.accounts[&node_bond_escrow_address(&[2; 32])].clone();
    let c = counterfeit(&s);
    apply(&mut s, report(vec![c.clone()], 1), f::H).unwrap();
    assert!(jailed(&s, 1));
    assert!(!jailed(&s, 2));
    assert_eq!(s.accounts[&node_bond_escrow_address(&[2; 32])], victim);
    let escrow = &s.accounts[&node_bond_escrow_address(&[1; 32])];
    assert_eq!(
        escrow.balance.0, 0,
        "a log without actual debit is not confiscation"
    );
    assert_eq!(escrow.stake_locked.0, 0);
    assert_eq!(escrow.spendable_balance.0, 0);
    let sink = &s.accounts[&forfeiture_address()];
    assert_eq!(sink.balance.0, u128::from(NODE_BOND_UNITS));
    assert_eq!(sink.balance, sink.stake_locked);
    assert_eq!(sink.balance, sink.stake_required);
    assert_eq!(sink.spendable_balance.0, 0);
    assert_eq!(s.total_minted_supply(), initial);
    assert_eq!(s.supply_authorized_minting, minted);
    assert_eq!(s.supply_authorized_burning, burned);
    s.validate_node_bond_anchor(f::H).unwrap();
    let sink_before = sink.clone();
    apply(&mut s, report(vec![c], 2), f::H).unwrap();
    assert_eq!(s.accounts[&forfeiture_address()], sink_before);
    assert_eq!(s.total_minted_supply(), initial);
    assert_eq!(s.node_fraud.as_ref().unwrap().terminals.len(), 1);
}

#[test]
fn equivocation_requires_same_ticket_and_attributes_each_authenticated_signer() {
    let mut s = f::state();
    let a = f::claim(&s, 1);
    let other_ticket = f::claim(&s, 2);
    let before = s.state_root_material();
    assert!(apply(&mut s, report(vec![a.clone(), other_ticket], 1), f::H).is_err());
    assert!(apply(&mut s, report(vec![a.clone(), a.clone()], 1), f::H).is_err());
    assert_eq!(s.state_root_material(), before);
    let mut b = a.clone();
    b.manifest.payload.tip_hash_hex = hex::encode([66; 32]);
    f::resign_claim(&mut b, 11, 22);
    let mut server_only = b.clone();
    server_only.consumer_signature.clear();
    let mut only = s.clone();
    apply(&mut only, report(vec![a.clone(), server_only], 1), f::H).unwrap();
    assert!(jailed(&only, 1));
    assert!(!jailed(&only, 2));
    let mut consumer_only_a = a.clone();
    consumer_only_a.server_signature.clear();
    let mut consumer_only_b = b.clone();
    consumer_only_b.server_signature.clear();
    let mut only = s.clone();
    apply(
        &mut only,
        report(vec![consumer_only_a, consumer_only_b], 1),
        f::H,
    )
    .unwrap();
    assert!(!jailed(&only, 1));
    assert!(jailed(&only, 2));
    let supply = s.total_minted_supply();
    apply(&mut s, report(vec![a, b], 1), f::H).unwrap();
    assert!(jailed(&s, 1));
    assert!(jailed(&s, 2));
    assert_eq!(
        s.accounts[&forfeiture_address()].balance.0,
        2 * u128::from(NODE_BOND_UNITS)
    );
    assert_eq!(s.total_minted_supply(), supply);
}

#[test]
fn inclusive_challenge_deadline_then_late_proof_refusal_and_slashed_maturity_unissued() {
    let mut s = f::state();
    let c = counterfeit(&s);
    let valid = f::claim(&s, 2);
    f::apply(&mut s, f::submission(vec![valid], 1)).unwrap();
    let close = epoch_close(f::bonds::A, 1).unwrap();
    let deadline = close + TESTNET_CHALLENGE_BLOCKS;
    advance(&mut s, deadline - 1);
    s.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: deadline,
        parent_hash: [3; 32],
    })
    .unwrap();
    let original = s.node_settlement.as_ref().unwrap().epochs[&1].clone();
    assert_eq!(original.status, EpochStatus::Provisional);
    let mut late = s.clone();
    advance(&mut late, deadline + 1);
    let before = late.state_root_material();
    assert!(apply(&mut late, report(vec![c.clone()], 2), deadline + 1).is_err());
    assert_eq!(late.state_root_material(), before);
    apply(&mut s, report(vec![c], 2), deadline).unwrap();
    let supply = s.total_minted_supply();
    advance(&mut s, deadline + 1);
    let epoch = &s.node_settlement.as_ref().unwrap().epochs[&1];
    assert_eq!(epoch.status, EpochStatus::Unissued);
    assert!(
        epoch.payouts.is_empty(),
        "slashed node must never mature a payment"
    );
    assert_eq!(epoch.unissued, epoch.scheduled);
    assert_eq!(s.total_minted_supply(), supply);
    let entries = export_state_entries(&s).unwrap();
    let restored = import_state_entries(&State::new(), &entries).unwrap();
    assert_eq!(s.state_root, restored.state_root);
    assert_eq!(s.node_fraud, restored.node_fraud);
    assert_eq!(s.accounts, restored.accounts);
    let mut missing = entries.clone();
    missing.retain(|(key, _)| key != b"state/node_fraud");
    assert!(import_state_entries(&State::new(), &missing).is_err());
    let mut mutated = restored.clone();
    mutated.node_fraud.as_mut().unwrap().confiscated += 1;
    assert_ne!(s.state_root_material(), mutated.state_root_material());
}

#[test]
fn jail_at_close_withholds_eligibility_and_refuses_lifecycle_after_any_slash() {
    let mut s = f::state();
    let honest = f::claim(&s, 2);
    let proof = counterfeit(&s);
    f::apply(&mut s, f::submission(vec![honest], 1)).unwrap();
    apply(&mut s, report(vec![proof], 2), f::H).unwrap();
    let close = epoch_close(f::bonds::A, 1).unwrap();
    advance(&mut s, close);
    assert_eq!(
        s.node_settlement.as_ref().unwrap().epochs[&1].status,
        EpochStatus::Unissued
    );
    let before = s.state_root_material();
    for action in [
        NodeBondAction::Update,
        NodeBondAction::Exit,
        NodeBondAction::Withdraw,
    ] {
        assert!(f::bonds::apply(&mut s, f::bonds::auth(10, 1, 3, action, 2), close).is_err());
        assert_eq!(s.state_root_material(), before);
    }
}

#[test]
fn forfeiture_account_refuses_generic_spend_release_and_credits_atomically() {
    let mut s = f::state();
    let proof = counterfeit(&s);
    apply(&mut s, report(vec![proof], 1), f::H).unwrap();
    let before = s.state_root_material();
    let sink = forfeiture_address();
    let spend = StateTransition::BalanceTransfer {
        from: sink,
        to: f::bonds::address(20),
        amount: Balance::new(1),
        nonce: Some(0),
        current_block: f::H,
    };
    assert!(s.apply_transition(spend).is_err());
    assert_eq!(s.state_root_material(), before);
    let release = StateTransition::ReleaseToSpendable {
        address: sink,
        amount: Balance::new(1),
    };
    assert!(
        s.apply_transition(release).is_err(),
        "ordinary release must not unlock forfeiture for a later spend"
    );
    assert_eq!(s.state_root_material(), before);
    // A protocol reward could otherwise make previously locked money spendable.
    let credit = StateTransition::BalanceTransfer {
        from: f::bonds::address(20),
        to: sink,
        amount: Balance::new(1),
        nonce: Some(1),
        current_block: f::H,
    };
    assert!(s.apply_transition(credit).is_err());
    assert_eq!(s.state_root_material(), before);
}

#[test]
fn report_purpose_binding_bounds_and_block_input_identity_fail_closed() {
    use wrfcoin_shared_protocol::blockchain::{block::Block, transaction::TransactionType};
    let mut s = f::state();
    let c = counterfeit(&s);
    let p = report(vec![c.clone()], 1);
    let mut tx = f::transaction(&p);
    tx.tx_type = TransactionType::NodeFraudReport;
    tx.set_id();
    assert_eq!(parse(&tx).unwrap(), p);
    let admission = f::submission(vec![c.clone()], 1);
    assert!(verify_authorization(&admission, &f::identity(), &[7; 32]).is_err());
    assert!(p.verify_authorization(&f::identity(), &[7; 32]).is_err());
    assert!(apply(&mut s, report(vec![c.clone(), c.clone(), c], 1), f::H).is_err());
    let mut block = Block::new(
        1,
        wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"fraud-parent"),
        vec![tx],
        1,
        f::H,
    );
    let transition = StateTransition::ApplyNodeFraudReport {
        payload: p.clone(),
        block_height: f::H,
    };
    assert!(validate_block_inputs(&block, &[transition.clone()], true).is_ok());
    assert!(validate_block_inputs(&block, &[], true).is_err());
    assert!(validate_block_inputs(&block, &[transition], false).is_err());
    block.transactions[0].data = Some("00".repeat(MAX_NODE_FRAUD_BYTES + 1));
    assert!(parse(&block.transactions[0]).is_none());
}

#[test]
fn chain_derived_mainnet_deadline_and_partial_withholding_never_redistribute() {
    let mut s = f::state();
    let c = counterfeit(&s);
    let p = report(vec![c], 1);
    let close = epoch_close(f::bonds::A, 1).unwrap();
    let mut mainnet_geometry = s.clone();
    mainnet_geometry
        .genesis_emission_config
        .as_mut()
        .unwrap()
        .chain_id = "1".into();
    assert!(
        responsible(
            &mainnet_geometry,
            &p,
            close + MAINNET_CHALLENGE_BLOCKS,
            &f::identity()
        )
        .is_ok()
    );
    assert!(
        responsible(
            &mainnet_geometry,
            &p,
            close + MAINNET_CHALLENGE_BLOCKS + 1,
            &f::identity()
        )
        .is_err()
    );
    let mut a = f::claim(&s, 2);
    let mut b = f::claim(&s, 3);
    b.ticket.server_id = [2; 32];
    b.ticket.consumer_id = [1; 32];
    b.receipt.receipt.server_id = "node-2".into();
    b.receipt.receipt.consumer_pubkey = f::pk(12);
    b.manifest.signer_id = hex::encode(f::pk(21));
    f::resign_claim(&mut b, 21, 12);
    f::resign_claim(&mut a, 11, 22);
    f::apply(&mut s, f::submission(vec![a], 1)).unwrap();
    let mut second = f::submission(vec![b], 1);
    second.authorization.sender = f::bonds::address(20);
    second.owner_key = f::pk(20);
    second.owner_signature = f::sign(&second.authorization, 20);
    f::apply(&mut s, second).unwrap();
    advance(&mut s, close);
    let provisional = s.node_settlement.as_ref().unwrap().epochs[&1].clone();
    assert_eq!(provisional.provisional.len(), 2);
    advance(&mut s, close + 1);
    let c = counterfeit(&s);
    apply(&mut s, report(vec![c], 2), close + 1).unwrap();
    let supply = s.total_minted_supply();
    advance(&mut s, close + TESTNET_CHALLENGE_BLOCKS + 1);
    let paid = &s.node_settlement.as_ref().unwrap().epochs[&1];
    assert_eq!(paid.status, EpochStatus::Paid);
    assert_eq!(paid.payouts.len(), 1);
    assert_eq!(paid.payouts[&[2; 32]], provisional.provisional[&[2; 32]]);
    assert_eq!(
        s.total_minted_supply() - supply,
        provisional.provisional[&[2; 32]]
    );
    assert_eq!(
        paid.unissued,
        paid.scheduled - provisional.provisional[&[2; 32]]
    );
}

#[test]
fn actual_withdrawal_one_before_buffer_refused_boundary_allowed_and_after_slash_refused() {
    use wrfcoin_shared_protocol::consensus::node_bond_registry::{BondExitBoundary, epoch_start};
    let mut clean = f::state();
    let proof = counterfeit(&clean);
    f::bonds::apply(
        &mut clean,
        f::bonds::auth(10, 1, 1, NodeBondAction::Exit, 2),
        f::H,
    )
    .unwrap();
    let mut slashed = clean.clone();
    apply(&mut slashed, report(vec![proof], 2), f::H).unwrap();
    let exit = epoch_start(f::bonds::A, 2).unwrap();
    let boundary = BondExitBoundary::checked(f::bonds::A, exit, TESTNET_CHALLENGE_BLOCKS).unwrap();
    let early = boundary.earliest_withdrawal_height - 1;
    advance(&mut clean, early);
    let epoch = (early - f::bonds::A) / NODE_EPOCH_BLOCKS;
    let withdrawal = f::bonds::auth(10, 1, 2, NodeBondAction::Withdraw, epoch);
    let before = clean.state_root_material();
    assert!(f::bonds::apply(&mut clean, withdrawal.clone(), early).is_err());
    assert_eq!(clean.state_root_material(), before);
    let supply = clean.total_minted_supply();
    advance(&mut clean, early + 1);
    f::bonds::apply(&mut clean, withdrawal, early + 1).unwrap();
    assert_eq!(
        clean.accounts[&node_bond_escrow_address(&[1; 32])]
            .balance
            .0,
        0
    );
    assert_eq!(clean.total_minted_supply(), supply);
    advance(&mut slashed, early + 1);
    let mut withdrawal = f::bonds::auth(10, 1, 3, NodeBondAction::Withdraw, epoch);
    withdrawal.signed.authorization.amount_units = 0;
    f::bonds::resign(&mut withdrawal, 10);
    let before = slashed.state_root_material();
    assert!(f::bonds::apply(&mut slashed, withdrawal, early + 1).is_err());
    assert_eq!(slashed.state_root_material(), before);
}

#[test]
fn omitted_outer_attribution_cannot_establish_blame() {
    let mut s = f::state();
    let mut c = counterfeit(&s);
    c.server_signature.clear();
    let before = s.state_root_material();
    assert!(apply(&mut s, report(vec![c], 1), f::H).is_err());
    assert_eq!(s.state_root_material(), before);
    assert!(!jailed(&s, 1));
    assert!(!jailed(&s, 2));
}

#[test]
fn dust_prefunded_sink_activates_preserves_supply_and_is_nonspendable() {
    let configured = f::bonds::state();
    let mut s = State::new();
    s.genesis_emission_config = configured.genesis_emission_config.clone();
    s.feature_schedule_extensions = configured.feature_schedule_extensions.clone();
    s.create_account(
        f::bonds::address(10),
        Balance::new(u128::from(NODE_BOND_UNITS) * 3),
    );
    let sink = forfeiture_address();
    s.apply_transition(StateTransition::BalanceTransfer {
        from: f::bonds::address(10),
        to: sink,
        amount: Balance::new(1),
        nonce: Some(0),
        current_block: f::bonds::A - 1,
    })
    .unwrap();
    let supply = s.total_minted_supply();
    let minted = s.supply_authorized_minting;
    let burned = s.supply_authorized_burning;
    s.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: f::bonds::A,
        parent_hash: [4; 32],
    })
    .expect("prefunded sink must not halt activation");
    assert_eq!(s.accounts[&sink].balance.0, 1);
    assert_eq!(s.accounts[&sink].stake_locked.0, 1);
    assert_eq!(s.accounts[&sink].stake_required.0, 1);
    assert_eq!(s.accounts[&sink].spendable_balance.0, 0);
    assert_eq!(s.node_fraud.as_ref().unwrap().adopted, 1);
    assert_eq!(s.node_fraud.as_ref().unwrap().confiscated, 0);
    assert_eq!(s.total_minted_supply(), supply);
    assert_eq!(s.supply_authorized_minting, minted);
    assert_eq!(s.supply_authorized_burning, burned);
    s.validate_node_bond_anchor(f::bonds::A).unwrap();
    let before = s.state_root_material();
    assert!(
        s.apply_transition(StateTransition::ReleaseToSpendable {
            address: sink,
            amount: Balance::new(1)
        })
        .is_err()
    );
    assert_eq!(s.state_root_material(), before);
    let mut changed = s.clone();
    changed.node_fraud.as_mut().unwrap().adopted = 2;
    assert_ne!(
        changed.state_root_material(),
        before,
        "adopted forfeiture belongs in the root"
    );
    s.apply_block_finalization(f::bonds::A);
    s.recalculate_state_root();
    let restored = import_state_entries(&State::new(), &export_state_entries(&s).unwrap()).unwrap();
    assert_eq!(restored.node_fraud, s.node_fraud);
}

#[test]
fn sink_refuses_unstake_stake_release_withdraw_slash_delegation_and_block_credits() {
    use wrfcoin_shared_protocol::accounts::{
        authorization::{AccountAuthorization, AuthorizationRule, DeclareAuthorization},
        paymaster::PaymasterAction,
    };
    use wrfcoin_shared_protocol::common::types::Timestamp;
    let mut s = f::state();
    let c = counterfeit(&s);
    apply(&mut s, report(vec![c], 1), f::H).unwrap();
    let sink = forfeiture_address();
    let delegated = DeclareAuthorization {
        account: sink,
        authorization: Some(AccountAuthorization {
            declared: Some(AuthorizationRule::Delegated {
                program: [4; 32],
                valid_until: None,
            }),
        }),
        nonce: 0,
        generation: 0,
        signer_public_key: String::new(),
        signature: String::new(),
    };
    let cases = vec![
        (
            "unstake",
            StateTransition::ApplyPaymasterAction {
                action: PaymasterAction::RequestUnstake { paymaster: sink },
                block_height: f::H,
            },
        ),
        (
            "stake-release",
            StateTransition::ReleaseToSpendable {
                address: sink,
                amount: Balance::new(1),
            },
        ),
        (
            "stake",
            StateTransition::LockToStake {
                address: sink,
                amount: Balance::new(1),
            },
        ),
        (
            "withdraw",
            StateTransition::ApplyPaymasterAction {
                action: PaymasterAction::WithdrawStake { paymaster: sink },
                block_height: f::H,
            },
        ),
        (
            "slash",
            StateTransition::Slash {
                address: sink,
                amount: Balance::new(1),
                reason: "synthetic refusal".into(),
            },
        ),
        (
            "delegation",
            StateTransition::DeclareAuthorization {
                declaration: delegated,
                block_height: f::H,
                block_timestamp: Timestamp::new(0),
            },
        ),
        (
            "block-reward",
            StateTransition::ApplyReward {
                address: sink,
                amount: Balance::new(1),
                reason: "synthetic block reward".into(),
            },
        ),
    ];
    for (name, transition) in cases {
        let mut candidate = s.clone();
        let before = candidate.state_root_material();
        let error = candidate.apply_transition(transition).unwrap_err();
        if matches!(name, "unstake" | "withdraw" | "delegation") {
            assert!(
                matches!(&error, wrfcoin_shared_protocol::blockchain::state::StateError::Governance(reason)
                    if reason == "SP_NODE_FORFEITURE_PROTECTED"),
                "{name} must reach the sink guard, not another feature gate: {error:?}"
            );
        }
        assert_eq!(candidate.state_root_material(), before, "{name}");
    }
    let mut block_applier = s.clone();
    let root = block_applier.state_root_material();
    assert!(
        block_applier
            .apply_transitions_batch_at_height(
                f::H,
                vec![StateTransition::ApplyReward {
                    address: sink,
                    amount: Balance::new(1),
                    reason: "synthetic block reward".into()
                }]
            )
            .is_err()
    );
    assert_eq!(block_applier.state_root_material(), root);
    let before = s.accounts[&sink].clone();
    s.apply_block_finalization(f::H);
    assert_eq!(s.accounts[&sink], before);
}

#[test]
fn responsible_registration_cannot_exit_or_withdraw_during_evidence_window() {
    let mut s = f::state();
    let a = f::claim(&s, 71);
    let mut b = a.clone();
    b.manifest.payload.tip_hash_hex = hex::encode([66; 32]);
    f::resign_claim(&mut b, 11, 22);
    let p = report(vec![a, b], 1);
    let exit = f::bonds::auth(20, 2, 1, NodeBondAction::Exit, 2);
    f::bonds::apply(&mut s, exit, f::H).unwrap();
    let deadline = epoch_close(f::bonds::A, 1).unwrap() + TESTNET_CHALLENGE_BLOCKS;
    let exit_height =
        wrfcoin_shared_protocol::consensus::node_bond_registry::epoch_start(f::bonds::A, 2)
            .unwrap();
    let boundary =
        wrfcoin_shared_protocol::consensus::node_bond_registry::BondExitBoundary::checked(
            f::bonds::A,
            exit_height,
            TESTNET_CHALLENGE_BLOCKS,
        )
        .unwrap();
    assert!(boundary.earliest_withdrawal_height > deadline);
    advance(&mut s, deadline);
    let epoch = (deadline - f::bonds::A) / NODE_EPOCH_BLOCKS;
    let withdrawal = f::bonds::auth(20, 2, 2, NodeBondAction::Withdraw, epoch);
    assert!(f::bonds::apply(&mut s, withdrawal, deadline).is_err());
    assert!(
        s.node_bond_registry
            .as_ref()
            .unwrap()
            .registrations
            .contains_key(&[2; 32])
    );
    let mut missing = s.clone();
    missing
        .node_bond_registry
        .as_mut()
        .unwrap()
        .registrations
        .remove(&[2; 32]);
    let before = missing.state_root_material();
    assert!(apply(&mut missing, p.clone(), deadline).is_err());
    assert_eq!(
        missing.state_root_material(),
        before,
        "unreachable missing registration refuses atomically"
    );
    apply(&mut s, p, deadline).unwrap();
    assert!(jailed(&s, 1) && jailed(&s, 2));
}

#[test]
fn fraud_root_material_cannot_be_omitted_on_both_producer_and_applier() {
    let s = f::state();
    let mut producer = s.clone();
    let mut applier = s;
    let before = producer.state_root_material();
    producer.node_fraud.as_mut().unwrap().report_count = 1;
    applier.node_fraud.as_mut().unwrap().report_count = 1;
    assert_eq!(
        producer.state_root_material(),
        applier.state_root_material()
    );
    assert_ne!(
        producer.state_root_material(),
        before,
        "both-sides omission hides the fraud state change"
    );
}
