#[path = "common/node_receipt_fixture.rs"]
mod f;
use wrfcoin_shared_protocol::{
    blockchain::{
        state::{State, StateTransition},
        state_snapshot::{export_state_entries, import_state_entries},
    },
    common::types::{Address, Balance},
    consensus::{
        node_bond_registry::epoch_close,
        node_settlement::*,
        node_share_contracts::*,
        proof_of_publishing::{EmissionPartitionLedger, EmissionRole, MAX_SUPPLY},
    },
    crypto::chain_binding::GenesisEmissionConfig,
};

fn begin(s: &mut State, height: u64) {
    s.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: height,
        parent_hash: [(height % 251) as u8; 32],
    })
    .unwrap();
}
fn finish(s: &mut State, height: u64) {
    s.apply_block_finalization(height);
    assert!(s.supply_violation.is_none(), "{:?}", s.supply_violation);
    assert_eq!(
        s.emission_ledger.cached_supply(),
        Some(s.checked_total_minted_supply().unwrap())
    );
}
fn funded() -> State {
    let mut s = f::bonds::state();
    s.genesis_emission_config = Some(GenesisEmissionConfig {
        chain_id: "777".into(),
        faucet_address: None,
     ..Default::default() });
    for (owner, id) in [(10, 1), (20, 2)] {
        f::bonds::apply(
            &mut s,
            f::bonds::auth(owner, id, 0, NodeBondAction::Deposit, 1),
            f::bonds::A,
        )
        .unwrap();
    }
    finish(&mut s, f::bonds::A);
    for h in f::bonds::A + 1..f::H {
        begin(&mut s, h);
        finish(&mut s, h);
    }
    begin(&mut s, f::H);
    s
}
fn advance(s: &mut State, height: u64) {
    let start = s.node_bond_registry.as_ref().unwrap().block_height;
    finish(s, start);
    for h in start + 1..=height {
        begin(s, h);
        finish(s, h);
    }
}

#[test]
fn empty_epoch_is_unissued_and_never_carried_forward() {
    let mut s = funded();
    let r = &s.node_settlement.as_ref().unwrap().epochs[&0];
    assert_eq!(r.status, EpochStatus::Unissued);
    assert!(r.scheduled > 0);
    assert_eq!(r.unissued, r.scheduled);
    assert_eq!(s.outstanding_node_reserves(), 0);
    let supply = s.total_minted_supply();
    finish(&mut s, f::H);
    assert_eq!(s.total_minted_supply(), supply);
    let r = &s.node_settlement.as_ref().unwrap().epochs[&1];
    assert_eq!(r.reserved, s.emission_ledger.reserved(EmissionRole::Node));
    assert_eq!(s.emission_ledger.minted(EmissionRole::Node), 0);
}

#[test]
fn epoch_close_maturity_snapshot_restore_and_once_only_current_journal() {
    let mut s = funded();
    let c = f::claim(&s, 1);
    f::apply(&mut s, f::submission(vec![c], 1)).unwrap();
    let initial = s.total_minted_supply();
    let close = epoch_close(f::bonds::A, 1).unwrap();
    advance(&mut s, close);
    let r = s.node_settlement.as_ref().unwrap().epochs[&1].clone();
    assert_eq!(r.status, EpochStatus::Provisional);
    assert_eq!(r.eligible.len(), 1);
    assert_eq!(r.provisional.values().sum::<u128>(), r.reserved);
    assert_eq!(s.total_minted_supply(), initial);
    assert_eq!(r.maturity_height, close + TESTNET_CHALLENGE_BLOCKS + 1);
    let entries = export_state_entries(&s).unwrap();
    let mut restored = import_state_entries(&State::new(), &entries).unwrap();
    assert_eq!(s.state_root, restored.state_root);
    assert_eq!(
        restored.emission_ledger.cached_supply(),
        Some(s.total_minted_supply())
    );
    assert_eq!(
        restored.emission_ledger.reserved(EmissionRole::Node),
        s.emission_ledger.reserved(EmissionRole::Node)
    );
    assert_eq!(restored.emission_ledger.minted(EmissionRole::Node), 0);
    advance(&mut s, r.maturity_height - 1);
    advance(&mut restored, r.maturity_height - 1);
    assert_eq!(
        s.total_minted_supply(),
        initial,
        "inclusive challenge deadline cannot mint"
    );
    let journal = s.supply_authorized_minting;
    advance(&mut s, r.maturity_height);
    advance(&mut restored, r.maturity_height);
    assert_eq!(
        s.state_root, restored.state_root,
        "restore/replay derives the identical reserve fold"
    );
    assert_eq!(s.total_minted_supply(), initial + r.reserved);
    assert_eq!(s.supply_authorized_minting - journal, r.reserved);
    assert_eq!(
        s.emission_ledger.minted(EmissionRole::Node),
        0,
        "old reserve is not charged to the new slice"
    );
    let after = s.state_root_material();
    finish(&mut s, r.maturity_height);
    assert_eq!(
        s.state_root_material(),
        after,
        "same-height finalization cannot redeem twice"
    );
    assert_eq!(s.supply_authorized_minting - journal, r.reserved);
    assert!(
        r.reserved > s.emission_ledger.partition().emission(),
        "cash redemption may exceed current E(h), prior authorization covers it"
    );
    assert!(s.emission_ledger.total_spent() <= s.emission_ledger.partition().emission());
}

#[test]
fn reserves_reduce_every_mint_consumers_headroom_and_same_height_ledger_cannot_reset() {
    let mut s = funded();
    finish(&mut s, f::H);
    let reserve = s.outstanding_node_reserves();
    assert!(reserve > 0);
    let whale = Address::new([90; 20]);
    let existing = s.total_minted_supply();
    s.create_account(whale, Balance::new(MAX_SUPPLY - existing - reserve));
    s.emission_ledger
        .set_cached_supply(s.checked_total_minted_supply().unwrap());
    assert_eq!(s.mint_headroom(), 0);
    let before = s.total_minted_supply();
    s.apply_transition(StateTransition::ApplyReward {
        address: f::bonds::address(10),
        amount: Balance::new(1),
        reason: "coinbase".into(),
    })
    .unwrap();
    assert_eq!(
        s.total_minted_supply(),
        before,
        "old generic mint cannot spend reserved headroom"
    );
    let spent = s.emission_ledger.spent(EmissionRole::Node);
    s.open_emission_block(f::H);
    assert_eq!(
        s.emission_ledger.spent(EmissionRole::Node),
        spent,
        "opening same height cannot reset authorization"
    );
}

#[test]
fn max_supply_minus_one_with_old_reserve_preserves_existing_role_precedence() {
    let mut s = funded();
    finish(&mut s, f::H);
    let reserve = s.outstanding_node_reserves();
    let existing = s.total_minted_supply();
    s.create_account(
        Address::new([90; 20]),
        Balance::new(MAX_SUPPLY - existing - reserve - 1),
    );
    begin(&mut s, f::H + 1);
    assert_eq!(s.mint_headroom(), 1);
    let before = s.total_minted_supply();
    s.apply_transition(StateTransition::ApplyReward {
        address: f::bonds::address(10),
        amount: Balance::new(1),
        reason: "coinbase".into(),
    })
    .unwrap();
    assert_eq!(s.total_minted_supply(), before + 1);
    // The synthetic whale is an externally seeded baseline for this control.
    s.supply_baseline_initialized = false;
    finish(&mut s, f::H + 1);
    assert_eq!(
        s.outstanding_node_reserves(),
        reserve,
        "existing role takes the last free unit before Node reservation"
    );
    assert_eq!(s.total_minted_supply() + reserve, MAX_SUPPLY);
}

#[test]
fn exact_remainder_rotates_by_epoch_and_maturity_constants_are_chain_specific() {
    let ids = vec![[1; 32], [2; 32], [3; 32]];
    assert_eq!(
        rotated_split(0, 1001, ids.clone().into_iter())
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![334, 334, 333]
    );
    assert_eq!(
        rotated_split(1, 1001, ids.clone().into_iter())
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![333, 334, 334]
    );
    assert_eq!(
        rotated_split(2, 1001, ids.into_iter())
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![334, 333, 334]
    );
    let close = epoch_close(f::bonds::A, 1).unwrap();
    assert_eq!(
        maturity_height(f::bonds::A, 1, "777"),
        Some(close + 1024 + 1)
    );
    assert_eq!(
        maturity_height(f::bonds::A, 1, "wrfcoin-main"),
        Some(close + 4096 + 1)
    );
}

#[test]
fn duplicate_and_owner_caps_are_atomic_and_new_block_reopens_owner_capacity() {
    let mut s = funded();
    let c = f::claim(&s, 1);
    f::apply(&mut s, f::submission(vec![c.clone()], 1)).unwrap();
    let before = s.state_root_material();
    assert!(f::apply(&mut s, f::submission(vec![c], 2)).is_err());
    assert_eq!(s.state_root_material(), before);
    let c = f::claim(&s, 2);
    f::apply(&mut s, f::submission(vec![c], 2)).unwrap();
    let before = s.state_root_material();
    let c = f::claim(&s, 3);
    let p = f::submission(vec![c], 3);
    assert!(f::apply(&mut s, p.clone()).is_err());
    assert_eq!(s.state_root_material(), before);
    finish(&mut s, f::H);
    begin(&mut s, f::H + 1);
    s.apply_transition(StateTransition::ApplyNodeServiceBatch {
        payload: p,
        block_height: f::H + 1,
    })
    .unwrap();
    assert_eq!(
        s.node_receipt_registry.as_ref().unwrap().owner_batch_counts[&f::bonds::address(10)],
        1
    );
}

#[test]
fn incomplete_prior_activation_is_refused_without_mutation() {
    for receipt_missing in [false, true] {
        let mut s = funded();
        finish(&mut s, f::H);
        s.node_settlement = None;
        if receipt_missing {
            s.node_receipt_registry = None;
        }
        let before = s.state_root_material();
        assert!(
            s.apply_transition(StateTransition::BeginNodeServiceBlock {
                block_height: f::H + 1,
                parent_hash: [4; 32]
            })
            .is_err()
        );
        assert_eq!(s.state_root_material(), before);
        assert!(s.validate_node_bond_anchor(f::H).is_err());
    }
}

#[test]
fn ownership_is_checked_before_invalid_inner_signature() {
    let mut s = funded();
    let mut c = f::claim(&s, 1);
    c.consumer_signature = vec![0; 64];
    let mut p = f::submission(vec![c], 0);
    p.authorization.sender = f::bonds::address(30);
    p.owner_key = f::pk(30);
    p.owner_signature = f::sign(&p.authorization, 30);
    s.create_account(f::bonds::address(30), Balance::new(0));
    let err = f::apply(&mut s, p).unwrap_err();
    assert!(err.to_string().contains("SP_NODE_RECEIPT_OWNER"), "{err}");
}

#[test]
fn retained_claim_size_checks_match_frozen_wire_bounds_and_report_debug_cost() {
    use std::time::Instant;
    use wrfcoin_shared_protocol::consensus::node_receipt_registry::{
        AcceptedNodeClaim, NodeReceiptRegistry,
    };
    let s = funded();
    let claim = f::claim(&s, 1);
    claim.validate_wire_bounds().unwrap();
    let mut oversized = claim.clone();
    oversized.manifest.nonce = "x".repeat(2000);
    assert!(oversized.to_wire().is_err());
    assert!(oversized.validate_wire_bounds().is_err());
    // Synthetic retained transport shape only: no assertion of valid service,
    // bootstrap acceptance, or activation readiness comes from this timing.
    let mut registry = NodeReceiptRegistry::default();
    for i in 0..1400u64 {
        let mut key = [0; 32];
        key[..8].copy_from_slice(&i.to_le_bytes());
        registry.accepted.insert(
            key,
            AcceptedNodeClaim {
                included_height: f::H,
                claim: claim.clone(),
            },
        );
    }
    let bytes = bincode::serialize(&registry).unwrap().len();
    let mut pressure = s.clone();
    pressure.node_receipt_registry = Some(registry.clone());
    let entries = export_state_entries(&pressure).unwrap();
    let json_bytes = serde_json::to_vec(&entries).unwrap().len();
    let t = Instant::now();
    for _ in 0..20 {
        std::hint::black_box(registry.clone());
    }
    let clone_us = t.elapsed().as_micros() / 20;
    let t = Instant::now();
    for _ in 0..20 {
        for record in registry.accepted.values() {
            record.claim.validate_wire_bounds().unwrap();
        }
    }
    let bounds_us = t.elapsed().as_micros() / 20;
    eprintln!(
        "S4_SYNTHETIC_DEBUG_COST claims=1400 bincode_bytes={bytes} snapshot_entries_json_bytes={json_bytes} clone_us={clone_us} bounds_walk_us={bounds_us}"
    );
    assert!(
        json_bytes > 2 * 1024 * 1024,
        "synthetic entries alone exceed the bonded-ingest 2 MiB request limit"
    );
}

#[test]
fn mainnet_recorded_identity_derives_maturity_and_retains_full_challenge_history() {
    use wrfcoin_shared_protocol::consensus::node_bond_registry::{
        MAX_RETAINED_NODE_EPOCHS, max_retained_node_epochs,
    };
    use wrfcoin_shared_protocol::crypto::domain_signing::DEFAULT_CHAIN_ID;
    // Pure state-fold geometry; does not authorize mainnet activation or bypass
    // admission. No receipt, registration or service-evidence claim is made.
    let mut s = f::bonds::state();
    s.genesis_emission_config.as_mut().unwrap().chain_id = DEFAULT_CHAIN_ID.into();
    s.node_settlement = Some(NodeSettlement::default());
    s.emission_ledger = EmissionPartitionLedger::open(f::bonds::A, s.genesis_emission_chain_id());
    let challenge = node_challenge_blocks(s.genesis_emission_chain_id());
    assert_eq!(challenge, MAINNET_CHALLENGE_BLOCKS);
    let bound = max_retained_node_epochs(s.genesis_emission_chain_id());
    assert!(bound > MAX_RETAINED_NODE_EPOCHS);
    let end = f::bonds::A + NODE_EPOCH_BLOCKS * (bound as u64 + 1);
    let mut peak = 0;
    for h in f::bonds::A..=end {
        if h != f::bonds::A {
            s.node_bond_registry
                .as_mut()
                .unwrap()
                .advance_with_challenge(h, [(h % 251) as u8; 32], challenge)
                .unwrap();
        }
        s.node_receipt_registry
            .as_mut()
            .unwrap()
            .advance(s.node_bond_registry.as_ref().unwrap());
        s.node_fraud.as_mut().unwrap().advance(s.node_bond_registry.as_ref().unwrap());
        s.open_emission_block(h);
        finalize(&mut s, h).unwrap();
        let bonds = s.node_bond_registry.as_ref().unwrap();
        peak = peak.max(bonds.frozen_epochs.len());
        assert!(bonds.frozen_epochs.len() <= bound);
        assert!(s.node_settlement.as_ref().unwrap().epochs.len() <= bound + 1);
        for (e, budget) in &s.node_settlement.as_ref().unwrap().epochs {
            assert_eq!(
                budget.maturity_height,
                epoch_close(f::bonds::A, *e).unwrap() + MAINNET_CHALLENGE_BLOCKS + 1
            );
        }
        s.validate_node_bond_anchor(h).unwrap();
    }
    assert!(
        peak > MAX_RETAINED_NODE_EPOCHS,
        "mainnet must preserve more epochs than the testnet pin"
    );
}

#[test]
fn cumulative_multi_epoch_issuance_and_reserves_never_exceed_scheduled_emission_or_supply() {
    use wrfcoin_shared_protocol::consensus::node_bond_registry::{epoch_at, epoch_start};
    let mut s = funded();
    let initial_supply = s.total_minted_supply();
    let mut cumulative_emission = 0u128;
    let mut nonce = 1;
    let mut paid_epochs = std::collections::BTreeSet::new();
    let template = f::claim(&s, 1);
    let end = maturity_height(f::bonds::A, 3, s.genesis_emission_chain_id().unwrap()).unwrap();
    for h in f::H..=end {
        if h != f::H {
            begin(&mut s, h);
        }
        let epoch = epoch_at(f::bonds::A, h).unwrap();
        if epoch <= 3 && Some(h) == epoch_start(f::bonds::A, epoch) {
            let frozen = &s.node_bond_registry.as_ref().unwrap().frozen_epochs[&epoch];
            let mut c = template.clone();
            c.ticket.context.epoch = epoch;
            c.ticket.context.roster_sequence = frozen.roster_sequence;
            c.ticket.context.roster_hash =
                wrfcoin_shared_protocol::consensus::node_receipt_registry::frozen_roster_hash(
                    frozen,
                );
            c.ticket.context.epoch_start_parent = frozen.parent_hash;
            c.ticket.ticket_id = [epoch as u8; 32];
            c.receipt.receipt.chain_height = h;
            c.manifest.payload.height = h;
            c.manifest.payload.finalized_height = h;
            f::resign_claim(&mut c, 11, 22);
            s.apply_transition(StateTransition::ApplyNodeServiceBatch {
                payload: f::submission(vec![c], nonce),
                block_height: h,
            })
            .unwrap();
            nonce += 1;
        }
        s.apply_transition(StateTransition::ApplyReward {
            address: Address::new([91; 20]),
            amount: Balance::new(1),
            reason: "coinbase".into(),
        })
        .unwrap();
        cumulative_emission = cumulative_emission
            .checked_add(s.emission_ledger.partition().emission())
            .unwrap();
        finish(&mut s, h);
        let cumulative_cash = s.total_minted_supply().checked_sub(initial_supply).unwrap();
        let committed = cumulative_cash
            .checked_add(s.outstanding_node_reserves())
            .unwrap();
        assert!(
            committed <= cumulative_emission,
            "height {h}: issued + outstanding exceed cumulative scheduled E(h)"
        );
        assert!(committed <= MAX_SUPPLY);
        assert!(s.total_minted_supply() + s.outstanding_node_reserves() <= MAX_SUPPLY);
        assert!(s.emission_ledger.total_spent() <= s.emission_ledger.partition().emission());
        for (e, r) in &s.node_settlement.as_ref().unwrap().epochs {
            if r.status == EpochStatus::Paid {
                paid_epochs.insert(*e);
            }
        }
    }
    assert_eq!(paid_epochs, std::collections::BTreeSet::from([1, 2, 3]));
    assert!(
        s.total_minted_supply() - initial_supply > end as u128 - f::H as u128 + 1,
        "must observe real prior-reserve cash redemptions"
    );
}

fn one_unit_headroom() -> State {
    let mut s = funded();
    finish(&mut s, f::H);
    let reserve = s.outstanding_node_reserves();
    assert!(reserve > 1);
    s.create_account(
        Address::new([90; 20]),
        Balance::new(MAX_SUPPLY - s.total_minted_supply() - reserve - 1),
    );
    begin(&mut s, f::H + 1);
    assert_eq!(s.total_minted_supply() + reserve, MAX_SUPPLY - 1);
    assert_eq!(s.mint_headroom(), 1);
    s
}

fn quorum_for(s: &mut State, decision: &str) {
    use wrfcoin_shared_protocol::oracle::attestation::{AttestationRecord, AttestationSet};
    let mut set = AttestationSet::default();
    for address in [Address::new([81; 20]), Address::new([82; 20])] {
        set.add(AttestationRecord {
            oracle_id: address.to_string(),
            approved: true,
            confidence: 1.0,
            timestamp: 0,
        });
    }
    assert!(set.has_quorum(s.oracle_quorum.max(1)));
    s.attestations.insert(decision.into(), set);
}

#[test]
fn faucet_preserves_outstanding_reserve_at_max_supply_minus_one() {
    let mut s = one_unit_headroom();
    let sink = Address::new([80; 20]);
    s.genesis_emission_config.as_mut().unwrap().faucet_address = Some(sink);
    let supply = s.total_minted_supply();
    s.apply_transition(StateTransition::ApplyFaucetReward)
        .unwrap();
    assert_eq!(s.total_minted_supply(), supply + 1);
    assert_eq!(s.emission_ledger.minted(EmissionRole::Faucet), 1);
    assert_eq!(s.accounts[&sink].balance.0, 1);
    assert_eq!(s.mint_headroom(), 0);
}

#[test]
fn contribution_preserves_outstanding_reserve_at_max_supply_minus_one() {
    use wrfcoin_shared_protocol::{
        blockchain::state::ContributionRewardRecord, station_registry::ContributionProvenanceClass,
    };
    let mut s = one_unit_headroom();
    let decision = "reserve-contribution";
    quorum_for(&mut s, decision);
    let mut record = ContributionRewardRecord {
        address: Address::new([80; 20]),
        amount: Balance::new(2),
        reason: format!("contribution:{decision}"),
        station_id: "synthetic-station".into(),
        provenance_class: ContributionProvenanceClass::Real,
    };
    let supply = s.total_minted_supply();
    s.apply_transition(StateTransition::ApplyContributionRewardDecision {
        record: record.clone(),
        current_block: f::H + 1,
    })
    .unwrap();
    assert_eq!(
        s.total_minted_supply(),
        supply,
        "non-coinbase contribution cannot consume reserved headroom"
    );
    assert!(!s.finalized_contribution_rewards.contains(decision));
    assert_eq!(s.emission_ledger.spent(EmissionRole::Contribution), 0);
    record.amount = Balance::new(1);
    s.apply_transition(StateTransition::ApplyContributionRewardDecision {
        record,
        current_block: f::H + 1,
    })
    .unwrap();
    assert_eq!(s.total_minted_supply(), supply + 1);
    assert!(s.finalized_contribution_rewards.contains(decision));
}

#[test]
fn gamification_preserves_outstanding_reserve_at_max_supply_minus_one() {
    let mut s = one_unit_headroom();
    let supply = s.total_minted_supply();
    for amount in [2, 1] {
        s.apply_transition(StateTransition::ApplyGamificationReward {
            address: Address::new([80; 20]),
            amount: Balance::new(amount),
            claim_id: "reserve-gamification".into(),
            current_block: f::H + 1,
        })
        .unwrap();
        if amount == 2 {
            assert_eq!(s.total_minted_supply(), supply);
            assert!(!s.gamification_claims_seen.contains("reserve-gamification"));
            assert_eq!(s.emission_ledger.spent(EmissionRole::Contribution), 0);
        }
    }
    assert_eq!(s.total_minted_supply(), supply + 1);
    assert!(s.gamification_claims_seen.contains("reserve-gamification"));
}

#[test]
fn attester_preserves_outstanding_reserve_at_max_supply_minus_one() {
    use wrfcoin_shared_protocol::oracle::attestation::AttesterSettlementDebt;
    let mut s = one_unit_headroom();
    let decision = "reserve-attester";
    quorum_for(&mut s, decision);
    s.attester_settlement_due.insert(
        decision.into(),
        AttesterSettlementDebt {
            crossing_height: f::H,
            eligible: vec![
                Address::new([81; 20]).to_string(),
                Address::new([82; 20]).to_string(),
            ],
        },
    );
    let records = s
        .derive_attester_settlements(
            &[decision.into()],
            f::H + 1,
            s.emission_ledger.remaining(EmissionRole::Attester),
        )
        .unwrap();
    let amount = records.iter().map(|r| r.total_amount()).sum::<u128>();
    assert!(amount > 1);
    let supply = s.total_minted_supply();
    let mut control = s.clone();
    s.apply_transition(StateTransition::ApplyAttesterSettlements {
        records: records.clone(),
    })
    .unwrap();
    assert_eq!(s.total_minted_supply(), supply);
    assert!(s.attester_settlement_due.contains_key(decision));
    assert!(!s.attester_settlements.contains_key(decision));
    assert_eq!(s.emission_ledger.spent(EmissionRole::Attester), 0);
    // The same earned settlement pays with sufficient unreserved headroom.
    let whale = control.accounts.get_mut(&Address::new([90; 20])).unwrap();
    whale.balance = whale.balance.checked_sub(Balance::new(amount)).unwrap();
    whale.spendable_balance = whale
        .spendable_balance
        .checked_sub(Balance::new(amount))
        .unwrap();
    control.restore_node_emission_runtime().unwrap();
    control
        .apply_transition(StateTransition::ApplyAttesterSettlements { records })
        .unwrap();
    assert_eq!(control.total_minted_supply(), supply);
    assert!(control.attester_settlements.contains_key(decision));
    assert_eq!(
        control.emission_ledger.spent(EmissionRole::Attester),
        amount
    );
}

#[test]
fn producer_preserves_outstanding_reserve_at_max_supply_minus_one() {
    use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
    let mut s = one_unit_headroom();
    let pk = f::pk(85);
    let recipient = Address::new([85; 20]);
    s.seed_genesis_producer_set(vec![ProducerMember {
        producer_id: "reserve-producer".into(),
        payout_address: recipient.to_string(),
        public_key: pk,
    }])
    .unwrap();
    let record = s.derive_producer_settlement(f::H + 1, &pk).unwrap();
    assert_eq!(record.amount, 1);
    let supply = s.total_minted_supply();
    s.apply_transition(StateTransition::ApplyProducerSettlement {
        producer_pubkey: pk,
        record,
    })
    .unwrap();
    assert_eq!(s.total_minted_supply(), supply + 1);
    assert_eq!(s.emission_ledger.spent(EmissionRole::Producer), 1);
    assert_eq!(s.accounts[&recipient].balance.0, 1);
    assert_eq!(s.mint_headroom(), 0);
    assert!(s.derive_producer_settlement(f::H + 1, &pk).is_none());
}
