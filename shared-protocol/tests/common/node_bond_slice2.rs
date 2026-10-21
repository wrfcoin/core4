use wrfcoin_shared_protocol::{
    blockchain::state::{State, StateTransition},
    common::types::{Address, Balance},
    consensus::{node_bond_registry::*, node_share_contracts::*},
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion},
        domain_signing::{DomainSeparated, sign_domain_for},
        signature::{PrivateKey, PublicKey},
    },
    governance::schedule_extension::{
        AppliedExtension, ScheduleEntry, install_schedule_extension_authority_for_tests,
    },
};

#[path = "node_bond_fixture.rs"]
mod helper;
pub use helper::*;
#[test]
fn node_bond_funds_real_principal_and_freezes_only_next_epoch() {
    assert_eq!(NODE_BOND_UNITS, 25_000_000_000);
    let mut s = state();
    let before = s.total_minted_supply();
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    assert_eq!(s.total_minted_supply(), before);
    assert_eq!(
        s.get_account(&address(10)).unwrap().balance.0,
        u128::from(NODE_BOND_UNITS) * 2
    );
    let escrow = s.get_account(&node_bond_escrow_address(&[1; 32])).unwrap();
    assert_eq!(escrow.balance.0, u128::from(NODE_BOND_UNITS));
    assert_eq!(escrow.stake_locked, escrow.balance);
    assert_eq!(escrow.spendable_balance.0, 0);
    assert!(
        s.node_bond_registry.as_ref().unwrap().frozen_epochs[&0]
            .bindings
            .is_empty()
    );
    advance(&mut s, A + NODE_EPOCH_BLOCKS);
    assert_eq!(
        s.node_bond_registry.as_ref().unwrap().frozen_epochs[&1]
            .bindings
            .len(),
        1
    );
    assert_eq!(
        s.node_bond_registry.as_ref().unwrap().frozen_epochs[&1].parent_hash,
        [((A + NODE_EPOCH_BLOCKS) % 251) as u8; 32]
    );
}
#[test]
fn node_bond_refuses_victim_unfunded_duplicate_and_bad_governance_atomically() {
    let mut s = state();
    let original = bincode::serialize(&s).unwrap();
    let mut victim = auth(10, 1, 0, NodeBondAction::Deposit, 1);
    victim.signed.authorization.owner_account = address(20);
    resign(&mut victim, 10);
    assert!(apply(&mut s, victim, A).is_err());
    assert_eq!(bincode::serialize(&s).unwrap(), original);
    let mut bad = auth(10, 1, 0, NodeBondAction::Deposit, 1);
    bad.governance_signature[0] ^= 1;
    assert!(apply(&mut s, bad, A).is_err());
    assert_eq!(bincode::serialize(&s).unwrap(), original);
    let mut victim_escrow = auth(10, 1, 0, NodeBondAction::Deposit, 1);
    victim_escrow.signed.authorization.escrow_account = address(20);
    // Re-sign all roles over the victim-targeting authorization.
    let a = &victim_escrow.signed.authorization;
    victim_escrow.signed.owner_signature = signature(a, 10);
    victim_escrow.governance_signature = signature(a, 99);
    victim_escrow.payout_signature = signature(a, 10);
    let digest = node_contract_digest(a, &ChainIdentity::new("777", "testnet"));
    victim_escrow.signed.server_signature = signature(
        &NodeBondPossession {
            authorization_preimage_hash: digest,
            role: NodeBondKeyRole::Server,
        },
        11,
    );
    victim_escrow.signed.consumer_signature = signature(
        &NodeBondPossession {
            authorization_preimage_hash: digest,
            role: NodeBondKeyRole::Consumer,
        },
        12,
    );
    assert!(apply(&mut s, victim_escrow, A).is_err());
    assert_eq!(bincode::serialize(&s).unwrap(), original);
    s.accounts.get_mut(&address(10)).unwrap().spendable_balance = Balance::new(1);
    assert!(apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).is_err());
    s.accounts.get_mut(&address(10)).unwrap().spendable_balance =
        Balance::new(u128::from(NODE_BOND_UNITS) * 3);
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let original = bincode::serialize(&s).unwrap();
    assert!(apply(&mut s, auth(10, 1, 1, NodeBondAction::Deposit, 1), A).is_err());
    assert_eq!(bincode::serialize(&s).unwrap(), original);
    let mut duplicate = auth(10, 1, 1, NodeBondAction::Deposit, 1);
    duplicate.signed.authorization.escrow_account = Address::new([9; 20]);
    resign(&mut duplicate, 10);
    assert!(apply(&mut s, duplicate, A).is_err());
    assert_eq!(bincode::serialize(&s).unwrap(), original);
    assert!(apply(&mut s, auth(10, 2, 1, NodeBondAction::Deposit, 1), A).is_err());
    assert_eq!(bincode::serialize(&s).unwrap(), original);
}
#[test]
fn node_bond_escrow_refuses_generic_unstake_and_transfer() {
    let mut s = state();
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let original = bincode::serialize(&s).unwrap();
    let escrow = node_bond_escrow_address(&[1; 32]);
    let amount = Balance::new(u128::from(NODE_BOND_UNITS));
    assert!(
        s.apply_transition(StateTransition::ReleaseToSpendable {
            address: escrow,
            amount
        })
        .is_err()
    );
    assert!(
        s.apply_transition(StateTransition::BalanceTransfer {
            from: escrow,
            to: address(20),
            amount,
            nonce: Some(0),
            current_block: A
        })
        .is_err()
    );
    assert!(
        s.apply_transition(StateTransition::LockToStake {
            address: escrow,
            amount: Balance::new(0)
        })
        .is_ok()
    );
    assert_eq!(bincode::serialize(&s).unwrap(), original);
}
#[test]
fn node_bond_exit_deadline_buffer_withdrawal_replay_and_nonce() {
    let mut s = state();
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    advance(&mut s, A + NODE_EPOCH_BLOCKS);
    apply(
        &mut s,
        auth(10, 1, 1, NodeBondAction::Exit, 2),
        A + NODE_EPOCH_BLOCKS,
    )
    .unwrap();
    let exit = A + 2 * NODE_EPOCH_BLOCKS;
    let b = BondExitBoundary::checked(A, exit, TESTNET_CHALLENGE_BLOCKS).unwrap();
    assert_eq!(b.last_service_epoch_close, 3074);
    assert_eq!(b.challenge_deadline, 4098);
    assert_eq!(b.earliest_withdrawal_height, 4199);
    for h in [
        b.challenge_deadline,
        b.challenge_deadline + NODE_BOND_WITHDRAWAL_BUFFER_BLOCKS,
    ] {
        advance(&mut s, h);
        let original = bincode::serialize(&s).unwrap();
        assert!(
            apply(
                &mut s,
                auth(10, 1, 2, NodeBondAction::Withdraw, epoch_at(A, h).unwrap()),
                h
            )
            .is_err()
        );
        assert_eq!(bincode::serialize(&s).unwrap(), original);
    }
    advance(&mut s, b.earliest_withdrawal_height);
    let h = b.earliest_withdrawal_height;
    let withdrawal = auth(10, 1, 2, NodeBondAction::Withdraw, epoch_at(A, h).unwrap());
    let before = s.total_minted_supply();
    let original = bincode::serialize(&s).unwrap();
    assert!(
        apply(
            &mut s,
            auth(10, 1, 0, NodeBondAction::Withdraw, epoch_at(A, h).unwrap()),
            h
        )
        .is_err(),
        "stale signed nonce cannot release principal"
    );
    assert_eq!(bincode::serialize(&s).unwrap(), original);
    apply(&mut s, withdrawal.clone(), h).unwrap();
    assert_eq!(s.total_minted_supply(), before);
    assert_eq!(
        s.get_account(&address(10)).unwrap().balance.0,
        u128::from(NODE_BOND_UNITS) * 3
    );
    let original = bincode::serialize(&s).unwrap();
    assert!(apply(&mut s, withdrawal, h).is_err());
    assert_eq!(bincode::serialize(&s).unwrap(), original);
}
#[test]
fn node_bond_roots_commit_every_registry_field_and_snapshot_round_trip() {
    let mut s = state();
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let root = s.state_root;
    let original = s.clone();
    for mutate in 0..12 {
        let mut changed = original.clone();
        let r = changed.node_bond_registry.as_mut().unwrap();
        match mutate {
            0 => r.activation_height += 1,
            1 => r.genesis_hash[0] ^= 1,
            2 => r.block_height += 1,
            3 => r.lifecycle_tx_count += 1,
            4 => r.frozen_epochs.get_mut(&0).unwrap().parent_hash[0] ^= 1,
            5 => r.registrations.get_mut(&[1; 32]).unwrap().principal_units -= 1,
            6 => r.registrations.get_mut(&[1; 32]).unwrap().last_change_epoch += 1,
            7 => {
                r.registrations
                    .get_mut(&[1; 32])
                    .unwrap()
                    .exit_effective_height = Some(9)
            }
            8 => {
                r.registrations
                    .get_mut(&[1; 32])
                    .unwrap()
                    .binding
                    .payout_account = address(20)
            }
            9 => r.registrations.get_mut(&[1; 32]).unwrap().registered_epoch += 1,
            10 => r.sequence += 1,
            _ => r.frozen_epochs.get_mut(&0).unwrap().roster_sequence += 1,
        };
        changed.recalculate_state_root();
        assert_ne!(changed.state_root, root, "omitted field {mutate}");
    }
    let entries =
        wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries(&s).unwrap();
    assert_eq!(
        entries
            .iter()
            .find(|(k, _)| k == b"meta/format_version")
            .unwrap()
            .1,
        b"6"
    );
    let mut restored = State::new();
    restored.apply_snapshot_entries(&entries).unwrap();
    assert_eq!(restored.state_root, root);
    assert_eq!(restored.node_bond_registry, s.node_bond_registry);
    let without = entries
        .into_iter()
        .filter(|(k, _)| k != b"state/node_bond_registry")
        .collect::<Vec<_>>();
    assert!(State::new().apply_snapshot_entries(&without).is_err());
}
#[test]
fn node_bond_default_off_root_marker_and_lifecycle_cap() {
    let mut off = State::new();
    off.recalculate_state_root();
    let root = off.state_root;
    off.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: A - 1,
        parent_hash: [0; 32],
    })
    .unwrap();
    assert_eq!(off.state_root, root);
    assert!(apply(&mut off, auth(10, 1, 0, NodeBondAction::Deposit, 1), 0).is_err());
    assert_eq!(off.state_root, root);
    let mut s = state();
    assert_eq!(MAX_NODE_BOND_LIFECYCLE_TXS_PER_BLOCK, 16);
    s.node_bond_registry.as_mut().unwrap().lifecycle_tx_count = 16;
    assert!(apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).is_err());
    s.node_bond_registry.as_mut().unwrap().lifecycle_tx_count = 15;
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    assert_eq!(
        s.node_bond_registry.as_ref().unwrap().lifecycle_tx_count,
        16
    );
    // Replaying begin-block cannot reset admission capacity.
    s.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: A,
        parent_hash: [2; 32],
    })
    .unwrap();
    assert_eq!(
        s.node_bond_registry.as_ref().unwrap().lifecycle_tx_count,
        16
    );
}

#[test]
fn node_bond_sixteen_real_registrations_per_block_cap() {
    let mut s = state();
    for i in 0..17u8 {
        s.create_account(
            address(10 + 4 * i),
            Balance::new(u128::from(NODE_BOND_UNITS)),
        );
    }
    for i in 0..16u8 {
        apply(
            &mut s,
            auth(10 + 4 * i, i + 1, 0, NodeBondAction::Deposit, 1),
            A,
        )
        .unwrap();
    }
    assert_eq!(
        s.node_bond_registry.as_ref().unwrap().registrations.len(),
        16
    );
    let original = bincode::serialize(&s).unwrap();
    assert!(apply(&mut s, auth(74, 17, 0, NodeBondAction::Deposit, 1), A).is_err());
    assert_eq!(bincode::serialize(&s).unwrap(), original);
    advance(&mut s, A + 1);
    apply(&mut s, auth(74, 17, 0, NodeBondAction::Deposit, 1), A + 1).unwrap();
}

#[test]
fn node_bond_mempool_height_gate_and_general_signature_byte_fee_policy() {
    use wrfcoin_shared_protocol::{
        config::settings::Config,
        consensus::verify_cost::{VerifyCost, VerifyPricing, minimum_fee_for},
        crypto::chain_binding::{FeatureSchedule, install_feature_schedule},
        mempool::pool::Mempool,
    };
    // Explicit synthetic ON fixture only; genesis/admission scheduling remains
    // held in production until all six reviewed slices are complete.
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("node_share_settlement".into(), A),
            ("account_authorization_dispatch".into(), A),
        ])
        .unwrap(),
    );
    let _context = state();
    let p = auth(10, 1, 0, NodeBondAction::Deposit, 1);
    let mut tx = transaction(&p, 0);
    assert_eq!(tx.verify_work().ed25519, 6);
    let mut config = Config::default();
    config.mempool_verify_fee_per_unit = 1;
    let required = minimum_fee_for(&tx, &VerifyPricing::default(), 1);
    assert_eq!(required, 600);
    let mut pool = Mempool::new(&config);
    assert!(
        pool.add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(A - 1))
            .is_err()
    );
    assert!(
        pool.add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(A))
            .is_err(),
        "underpriced verification work"
    );
    tx.fee = required;
    tx.set_id();
    pool.add_transaction_with_signing_keys_at_height(tx, None, Some(A))
        .unwrap();
}

#[test]
fn node_bond_rotation_is_next_epoch_and_historical_binding_is_frozen() {
    let mut s = state();
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    advance(&mut s, A + NODE_EPOCH_BLOCKS);
    let old = s.node_bond_registry.as_ref().unwrap().frozen_epochs[&1].clone();
    let mut update = auth(10, 1, 1, NodeBondAction::Update, 2);
    update.signed.authorization.service_alias = "rotated-alias".into();
    update.signed.authorization.server_key = pk(31);
    update.signed.authorization.consumer_key = pk(32);
    update.signed.authorization.payout_account = address(30);
    resign(&mut update, 10);
    update.payout_key = Some(pk(30));
    update.payout_signature = signature(&update.signed.authorization, 30);
    let digest = node_contract_digest(
        &update.signed.authorization,
        &ChainIdentity::new("777", "testnet"),
    );
    for (role, signer, target) in [
        (
            NodeBondKeyRole::Server,
            31,
            &mut update.signed.server_signature,
        ),
        (
            NodeBondKeyRole::Consumer,
            32,
            &mut update.signed.consumer_signature,
        ),
    ] {
        *target = signature(
            &NodeBondPossession {
                authorization_preimage_hash: digest,
                role,
            },
            signer,
        );
    }
    apply(&mut s, update, A + NODE_EPOCH_BLOCKS).unwrap();
    assert_eq!(
        s.node_bond_registry.as_ref().unwrap().frozen_epochs[&1],
        old
    );
    assert_eq!(
        s.node_bond_registry.as_ref().unwrap().registrations[&[1; 32]]
            .binding
            .service_alias,
        "node-1"
    );
    assert!(
        apply(
            &mut s,
            auth(10, 1, 2, NodeBondAction::Update, 2),
            A + NODE_EPOCH_BLOCKS
        )
        .is_err()
    );
    advance(&mut s, A + 2 * NODE_EPOCH_BLOCKS);
    let registry = s.node_bond_registry.as_ref().unwrap();
    assert_eq!(registry.frozen_epochs[&1], old);
    assert_eq!(
        registry.frozen_epochs[&2].bindings[&[1; 32]].service_alias,
        "rotated-alias"
    );
    assert_eq!(
        registry.frozen_epochs[&2].bindings[&[1; 32]].server_key,
        pk(31)
    );
    assert_eq!(
        registry.frozen_epochs[&2].bindings[&[1; 32]].consumer_key,
        pk(32)
    );
    assert_eq!(
        registry.frozen_epochs[&2].bindings[&[1; 32]].payout_account,
        address(30)
    );
    assert!(registry.registrations[&[1; 32]].pending_binding.is_none());
    let mut alias = auth(20, 2, 0, NodeBondAction::Deposit, 3);
    alias.signed.authorization.service_alias = "node-1".into();
    resign(&mut alias, 20);
    assert!(
        apply(&mut s, alias, A + 2 * NODE_EPOCH_BLOCKS).is_err(),
        "retained history reserves the former alias"
    );
}

#[test]
fn node_bond_escrow_refuses_generic_transfer_credit() {
    let mut s = state();
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let original = bincode::serialize(&s).unwrap();
    assert!(
        s.apply_transition(StateTransition::BalanceTransfer {
            from: address(20),
            to: node_bond_escrow_address(&[1; 32]),
            amount: Balance::new(1),
            nonce: Some(0),
            current_block: A
        })
        .is_err()
    );
    assert_eq!(bincode::serialize(&s).unwrap(), original);
}

#[test]
fn node_bond_owner_declared_policy_cannot_be_bypassed_by_wallet_key() {
    use wrfcoin_shared_protocol::accounts::authorization::{
        AccountAuthorization, AccountAuthorizationDeclaration, AuthorizationRule,
    };
    let mut s = state();
    s.authorization_declarations.insert(
        address(10),
        AccountAuthorizationDeclaration {
            next_nonce: 1,
            generation: 1,
            current: Some(AccountAuthorization::declaring(
                AuthorizationRule::SmartAccount { program: [42; 32] },
            )),
        },
    );
    s.recalculate_state_root();
    let original = bincode::serialize(&s).unwrap();
    assert!(apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).is_err());
    assert_eq!(bincode::serialize(&s).unwrap(), original);
}

#[test]
fn node_bond_online_keys_cannot_alias_other_node_wallets_in_either_direction() {
    let mut s = state();
    s.create_account(address(11), Balance::new(u128::from(NODE_BOND_UNITS)));
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let original = bincode::serialize(&s).unwrap();
    for (owner, server, consumer) in [(11, 21, 22), (20, 10, 22)] {
        let mut p = auth(owner, 2, 0, NodeBondAction::Deposit, 1);
        p.signed.authorization.server_key = pk(server);
        p.signed.authorization.consumer_key = pk(consumer);
        resign(&mut p, owner);
        let digest = node_contract_digest(
            &p.signed.authorization,
            &ChainIdentity::new("777", "testnet"),
        );
        p.signed.server_signature = signature(
            &NodeBondPossession {
                authorization_preimage_hash: digest,
                role: NodeBondKeyRole::Server,
            },
            server,
        );
        p.signed.consumer_signature = signature(
            &NodeBondPossession {
                authorization_preimage_hash: digest,
                role: NodeBondKeyRole::Consumer,
            },
            consumer,
        );
        assert!(
            apply(&mut s, p, A).is_err(),
            "registered online keys and other node wallets must stay separate"
        );
        assert_eq!(bincode::serialize(&s).unwrap(), original);
    }
}

#[test]
fn keyless_escrow_refuses_owner_choice_and_future_producer_collision() {
    let mut s = state();
    let mut chosen = auth(10, 1, 0, NodeBondAction::Deposit, 1);
    chosen.signed.authorization.escrow_account = Address::new([8; 20]);
    resign(&mut chosen, 10);
    let before = s.state_root_material();
    assert!(apply(&mut s, chosen, A).is_err());
    assert_eq!(s.state_root_material(), before);
    use wrfcoin_shared_protocol::consensus::producer_set::{ProducerSetChange, ProducerMember};
    s.producer_set_history.push(ProducerSetChange::new(1, A + 100,
        vec![ProducerMember { producer_id: "future".into(), payout_address: node_bond_escrow_address(&[1;32]).to_string(), public_key: pk(80) }]));
    let before = s.state_root_material();
    assert!(apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).is_err());
    assert_eq!(s.state_root_material(), before);
    assert_ne!(node_bond_escrow_address(&[1;32]), node_bond_escrow_address(&[2;32]));
}

#[test]
fn protocol_credit_configuration_cannot_claim_an_existing_escrow_3527() {
    use wrfcoin_shared_protocol::consensus::producer_set::{ProducerSetChange, ProducerMember};
    let mut s = state();
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let before = s.state_root_material();
    let member = ProducerMember { producer_id: "future-collision".into(), payout_address: node_bond_escrow_address(&[1;32]).to_string(), public_key: pk(80) };
    let change = ProducerSetChange::new(0, A + 100, vec![member]);
    assert!(s.apply_transition(StateTransition::ApplyProducerSetChange { change, block_height: A }).is_err());
    assert_eq!(s.state_root_material(), before);
    assert!(s.producer_set_history.is_empty());
    // Positive control: the same valid committee change with an independent
    // payout address is admitted; this does not blanket-disable governance.
    let member = ProducerMember { producer_id: "future-safe".into(), payout_address: address(80).to_string(), public_key: pk(80) };
    s.apply_transition(StateTransition::ApplyProducerSetChange { change: ProducerSetChange::new(0, A + 100, vec![member]), block_height: A }).unwrap();
}

#[test]
fn underfee_lifecycle_skips_signature_verification_3527() {
    use wrfcoin_shared_protocol::{config::settings::Config, mempool::pool::Mempool};
    let _context = state();
    let mut p = auth(10, 1, 0, NodeBondAction::Deposit, 1);
    p.signed.owner_signature[0] ^= 1;
    let mut config = Config::default();
    config.mempool_verify_fee_per_unit = 1;
    let mut pool = Mempool::new(&config);
    #[cfg(feature = "test-support")]
    let attempts = lifecycle_verify_attempts();
    let error = pool.add_transaction_with_signing_keys_at_height(transaction(&p, 0), None, Some(A)).unwrap_err();
    assert_eq!(error.to_string(), "transaction fee below verification-work admission minimum");
    #[cfg(feature = "test-support")]
    assert_eq!(lifecycle_verify_attempts(), attempts, "under-fee rejection must do zero lifecycle signature verification");
    // At the floor the same corrupted signature must reach the verifier.
    let error = pool.add_transaction_with_signing_keys_at_height(transaction(&p, 600), None, Some(A)).unwrap_err();
    assert_eq!(error.to_string(), "SP_NODE_BOND_AUTHORIZATION");
    #[cfg(feature = "test-support")]
    assert_eq!(lifecycle_verify_attempts(), attempts + 1);
}

#[test]
#[ignore = "bounded before/after timing; run explicitly with --ignored --nocapture"]
fn escrow_guard_timing_3527() {
    use std::{hint::black_box, time::Instant};
    let mut s = state();
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let prototype = s.node_bond_registry.as_ref().unwrap().registrations[&[1;32]].clone();
    for i in 2u32..=1000 {
        let mut id = [0;32]; id[..4].copy_from_slice(&i.to_le_bytes());
        let mut record = prototype.clone();
        record.binding.deployment_id = id;
        record.binding.escrow_account = node_bond_escrow_address(&id);
        s.accounts.insert(record.binding.escrow_account, s.accounts[&prototype.binding.escrow_account].clone());
        s.node_bond_registry.as_mut().unwrap().registrations.insert(id, record);
    }
    let start = Instant::now();
    for _ in 0..100 {
        black_box(s.apply_transition(StateTransition::LockToStake { address: address(20), amount: Balance::new(0) })).unwrap();
    }
    println!("ESCROW_GUARD_TIMING N=1000 transitions=100 elapsed_us={}", start.elapsed().as_micros());
}

#[test]
fn protocol_credit_sources_refuse_escrow_collisions_3527() {
    use wrfcoin_shared_protocol::{
        blockchain::state::ContributionRewardRecord,
        station_registry::ContributionProvenanceClass,
        oracle::attestation::AttesterSettlementDebt,
    };
    let escrow = node_bond_escrow_address(&[1;32]);
    let mut s = state();
    assert!(!is_protocol_credited_address(&s, &escrow));
    let mut accepted = Vec::new();
    for source in 0..10 {
        let mut collision = s.clone();
        match source {
            0 => collision.genesis_emission_config.as_mut().unwrap().faucet_address = Some(escrow),
            1 => { collision.oracle_set.insert(escrow.to_string()); },
            2 => { collision.attester_settlement_due.insert("deferred".into(), AttesterSettlementDebt { crossing_height: A, eligible: vec![escrow.to_string()] }); },
            3 => { collision.contribution_reward_records.insert("pending".into(), ContributionRewardRecord { address: escrow, amount: Balance::new(1), reason: "contribution:pending".into(), station_id: "station".into(), provenance_class: ContributionProvenanceClass::Real }); },
            4 => {
                collision.station_registry.register(wrfcoin_shared_protocol::station_registry::WeatherStation {
                    station_id: "collision-station".into(), owner_address: escrow.to_string(),
                    location: wrfcoin_shared_protocol::station_registry::StationLocation { latitude: 0., longitude: 0., elevation_meters: None },
                    hardware_model: "synthetic".into(), calibration_date: None, is_active: true,
                    reputation_score: 0, data_quality_tier: Default::default(), provenance_class: ContributionProvenanceClass::Real, ring_public_key: None,
                }).unwrap();
            }
            5 => {
                use wrfcoin_shared_protocol::consensus::producer_set::{ProducerSetChange, ProducerMember};
                collision.producer_set_history.push(ProducerSetChange::new(0, A + 100, vec![ProducerMember { producer_id: "synthetic".into(), payout_address: escrow.to_string(), public_key: pk(80) }]));
            }
            6..=8 => {
                apply(&mut collision, auth(10, 2, 0, NodeBondAction::Deposit, 1), A).unwrap();
                let registry = collision.node_bond_registry.as_mut().unwrap();
                let mut binding = registry.registrations[&[2;32]].binding.clone();
                binding.payout_account = escrow;
                match source {
                    6 => registry.registrations.get_mut(&[2;32]).unwrap().binding = binding,
                    7 => registry.registrations.get_mut(&[2;32]).unwrap().pending_binding = Some(binding),
                    _ => { registry.frozen_epochs.get_mut(&0).unwrap().bindings.insert([2;32], binding); },
                }
            }
            _ => {
                use wrfcoin_shared_protocol::consensus::node_settlement::{NodeEpochBudget, EpochStatus};
                collision.node_settlement.as_mut().unwrap().epochs.insert(0, NodeEpochBudget {
                    scheduled: 1, reserved: 1, unissued: 0, maturity_height: 10,
                    eligible: [([2;32], escrow)].into_iter().collect(),
                    payouts: Default::default(), provisional: [([2;32], 1)].into_iter().collect(), status: EpochStatus::Provisional,
                });
            }
        }
        let detected = is_protocol_credited_address(&collision, &escrow);
        let before = bincode::serialize(&collision).unwrap();
        let mut deposit = auth(20, 1, 0, NodeBondAction::Deposit, 1);
        deposit.signed.authorization.expected_nonce = 0;
        let refused = apply(&mut collision, deposit, A).is_err();
        if !detected || !refused { accepted.push(source); }
        if refused { assert_eq!(bincode::serialize(&collision).unwrap(), before); }
    }
    assert!(accepted.is_empty(), "protocol credit sources accepted collision: {accepted:?}");
    assert!(is_protocol_credited_address(&s, &wrfcoin_shared_protocol::consensus::node_fraud::forfeiture_address()));
    for row in wrfcoin_shared_protocol::crypto::chain_binding::LEGACY_FAUCET_BINDINGS {
        if let Some(address) = row.address { assert!(is_protocol_credited_address(&s, &address.parse().unwrap())); }
    }
    s.oracle_set.insert(hex::encode(pk(80)));
    assert!(is_protocol_credited_address(&s, &address(80)), "oracle public key encoding");
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    assert!(is_protocol_credited_address(&s, &address(10)), "node registration payout");
}

#[test]
fn targeted_escrow_guard_refuses_debits_before_any_side_effect_3527() {
    let mut s = state();
    apply(&mut s, auth(10, 1, 0, NodeBondAction::Deposit, 1), A).unwrap();
    let escrow = node_bond_escrow_address(&[1;32]);
    // A transition targeting escrow must refuse BEFORE debiting its ordinary
    // sender, reserving a nonce, or writing any auxiliary state.
    let before = bincode::serialize(&s).unwrap();
    for transition in [
        StateTransition::Slash { address: escrow, amount: Balance::new(1), reason: "synthetic-debit".into() },
        StateTransition::BalanceTransfer { from: address(20), to: escrow, amount: Balance::new(1), nonce: Some(0), current_block: A },
        StateTransition::ApplyReward { address: escrow, amount: Balance::new(1), reason: "synthetic-credit".into() },
    ] {
        assert!(s.apply_transition(transition).is_err());
        assert_eq!(bincode::serialize(&s).unwrap(), before);
    }
    s.apply_transition(StateTransition::BalanceTransfer { from: address(20), to: address(80), amount: Balance::new(1), nonce: Some(0), current_block: A }).unwrap();
}
