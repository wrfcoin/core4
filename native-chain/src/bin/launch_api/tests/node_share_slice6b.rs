//! Feature-ON production parity and Mango-down mutual service corpus.
use super::*;
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod f;
use wrfcoin_shared_protocol::{
    blockchain::{
        state::State,
        state_snapshot::{export_state_entries, import_state_entries},
    },
    common::types::Balance,
    consensus::{
        node_bond_registry::epoch_close,
        node_receipt_registry::{
            NodeReceiptSubmission, NodeReceiptSubmissionAuthorization, batch_hash,
        },
        node_settlement::{EpochStatus, maturity_height},
        node_share_contracts::{NodeBondAction, NodeServiceBatch},
        proof_of_publishing::EmissionRole,
    },
};

#[cfg(test)]
mod tests {
use super::*;
fn advance(s: &mut State, to: u64) {
    let from = s.node_bond_registry.as_ref().unwrap().block_height;
    s.apply_block_finalization(from);
    for h in from + 1..=to {
        s.apply_transition(StateTransition::BeginNodeServiceBlock {
            block_height: h,
            parent_hash: [4; 32],
        })
        .unwrap();
        s.apply_block_finalization(h);
        s.validate_node_finalized(h).unwrap();
        assert!(s.supply_violation.is_none());
    }
}
fn outage_state() -> State {
    let mut s = f::bonds::state();
    // Allocate the third fixture wallet from existing initial funds; no mint.
    let supply = s.total_minted_supply();
    let funding =
        u128::from(wrfcoin_shared_protocol::consensus::node_bond_registry::NODE_BOND_UNITS);
    let owner = s.accounts.get_mut(&f::bonds::address(10)).unwrap();
    owner.balance.0 -= funding;
    owner.spendable_balance.0 -= funding;
    s.create_account(f::bonds::address(30), Balance::new(funding));
    assert_eq!(s.total_minted_supply(), supply);
    s.restore_node_emission_runtime().unwrap();
    for (owner, id) in [(10, 1), (20, 2), (30, 3)] {
        f::bonds::apply(
            &mut s,
            f::bonds::auth(owner, id, 0, NodeBondAction::Deposit, 1),
            f::bonds::A,
        )
        .unwrap();
    }
    advance(&mut s, f::H);
    s
}
fn reverse_claim(
    s: &State,
) -> wrfcoin_shared_protocol::consensus::node_share_contracts::BondedServiceClaim {
    let mut c = f::claim(s, 2);
    c.ticket.server_id = [2; 32];
    c.ticket.consumer_id = [1; 32];
    c.receipt.receipt.server_id = "node-2".into();
    c.receipt.receipt.consumer_pubkey = f::pk(12);
    c.manifest.signer_id = hex::encode(f::pk(21));
    f::resign_claim(&mut c, 21, 12);
    c
}
fn submission(
    c: wrfcoin_shared_protocol::consensus::node_share_contracts::BondedServiceClaim,
    owner: u8,
    nonce: u64,
) -> NodeReceiptSubmission {
    let batch = NodeServiceBatch { claims: vec![c] };
    let authorization = NodeReceiptSubmissionAuthorization {
        genesis_hash: [7; 32],
        sender: f::bonds::address(owner),
        nonce,
        batch_hash: batch_hash(&batch).unwrap(),
    };
    NodeReceiptSubmission {
        owner_signature: f::sign(&authorization, owner),
        authorization,
        batch,
        owner_key: f::pk(owner),
    }
}
fn snapshot_parity(s: &State, height: u64) -> State {
    let entries = export_state_entries(s).unwrap();
    let mut restored = import_state_entries(&State::new(), &entries).unwrap();
    restored.validate_node_finalized(height).expect("snapshot import must retain valid marker and reserve");
    restored.recalculate_state_root();
    assert_eq!(
        restored.state_root, s.state_root,
        "snapshot imports the root marker and reserve"
    );
    assert_eq!(
        restored.node_settlement, s.node_settlement,
        "snapshot imports the complete reserve ledger"
    );
    assert_eq!(restored.node_bond_registry, s.node_bond_registry);
    assert_eq!(
        restored.emission_ledger.reserved(EmissionRole::Node),
        s.emission_ledger.reserved(EmissionRole::Node)
    );
    restored
}

#[cfg(feature = "p2p")]
#[actix_web::test]
async fn node_share_on_mining_peer_checkpoint_wal_snapshot_and_outage_parity() {
    const NAME: &str = "tests::node_share_slice6b::tests::node_share_on_mining_peer_checkpoint_wal_snapshot_and_outage_parity";
    if crate::helpers::test_env::isolated_chain_domain(
        NAME,
        "777",
        "testnet",
        "0x2727272727272727272727272727272727272727",
        "none",
    ) {
        return;
    }
    let _lock = crate::helpers::test_env::lock();
    let server = test_state(None);
    let mut state = outage_state();
    assert_eq!(
        state.node_bond_registry.as_ref().unwrap().frozen_epochs[&1]
            .bindings
            .len(),
        3
    );
    let claims = [(f::claim(&state, 1), 10), (reverse_claim(&state), 20)];
    {
        let mut chain = server.blockchain.write().await;
        chain.state = state.clone();
        chain.height = f::H;
        chain.tip = Hash::from_bytes([4; 32]);
    }
    for (claim, owner) in claims {
        let mut tx = f::transaction(&submission(claim, owner, 1));
        tx.inputs[0].txid = wrfcoin_shared_protocol::crypto::hash::hash_bytes(&[owner]);
        tx.set_id();
        server
            .mempool
            .write()
            .await
            .add_transaction_with_signing_keys_at_height(tx, None, Some(f::H))
            .unwrap();
    }
    let before = server.blockchain.read().await.clone();
    let block = server
        .consensus
        .create_pop_block_with_state(
            server.blockchain.clone(),
            server.mempool.clone(),
            &f::bonds::address(10).to_string(),
        )
        .await
        .unwrap();
    assert_eq!(
        block
            .transactions
            .iter()
            .filter(|t| t.tx_type == TransactionType::NodeServiceBatch)
            .count(),
        2,
        "mutual receipts must actually be mined"
    );
    state = server.blockchain.read().await.state.clone();
    let peer = crate::p2p_events::apply_node_receipt_block_for_test(before, &block).await;
    assert_eq!(
        peer.state_root, state.state_root,
        "receipt inclusion traverses actual P2P and WAL"
    );
    assert!(crate::p2p_events::diff_states(&peer, &state).is_empty());
    assert!(
        state
            .node_receipt_registry
            .as_ref()
            .unwrap()
            .has_service(1, &[1; 32])
    );
    assert!(
        state
            .node_receipt_registry
            .as_ref()
            .unwrap()
            .has_service(1, &[2; 32])
    );
    assert!(
        !state
            .node_receipt_registry
            .as_ref()
            .unwrap()
            .has_service(1, &[3; 32]),
        "Mango outage is no service"
    );
    state = snapshot_parity(&state, block.height);
    let mut roots = std::collections::BTreeMap::<String, String>::new();
    roots.insert(block.height.to_string(), hex::encode(state.state_root.0));
    let close = epoch_close(f::bonds::A, 1).unwrap();
    let maturity = maturity_height(f::bonds::A, 1, "777").unwrap();
    for boundary in [close, maturity] {
        advance(&mut state, boundary - 1);
        let replay_start = snapshot_parity(&state, boundary - 1);
        {
            let mut chain = server.blockchain.write().await;
            chain.state = state.clone();
            chain.height = boundary - 1;
            chain.tip = Hash::from_bytes([4; 32]);
        }
        let before = server.blockchain.read().await.clone();
        let block = server
            .consensus
            .create_pop_block_with_state(
                server.blockchain.clone(),
                server.mempool.clone(),
                &f::bonds::address(10).to_string(),
            )
            .await
            .unwrap();
        let produced = server.blockchain.read().await.state.clone();
        let peer =
            crate::p2p_events::apply_node_receipt_block_for_test(before.clone(), &block).await;
        assert_eq!(
            peer.state_root, produced.state_root,
            "actual mining to P2P settlement fold"
        );
        assert!(crate::p2p_events::diff_states(&peer, &produced).is_empty());
        let inputs = crate::transition_extractor::extract_transitions(
            &before.state,
            &block,
            block.height,
            block.timestamp.timestamp(),
        );
        server
            .storage
            .write()
            .await
            .force_checkpoint(&before.state, before.height)
            .await
            .unwrap();
        server
            .storage
            .write()
            .await
            .persist_block(block.clone(), inputs.clone(), None)
            .await
            .unwrap();
        let wal = server
            .storage
            .read()
            .await
            .load_state()
            .unwrap()
            .unwrap()
            .state;
        assert_eq!(
            wal.state_root, produced.state_root,
            "checkpoint plus durable WAL derives settlement"
        );
        server
            .storage
            .write()
            .await
            .force_checkpoint(&produced, boundary)
            .await
            .unwrap();
        let checkpoint = server
            .storage
            .read()
            .await
            .load_state()
            .unwrap()
            .unwrap()
            .state;
        assert_eq!(checkpoint.state_root, produced.state_root);
        let mut snapshot = snapshot_parity(&produced, boundary);
        assert_eq!(snapshot.node_settlement, checkpoint.node_settlement);
        let mut reorg = replay_start;
        reorg
            .apply_transitions_batch_at_height(boundary, inputs)
            .unwrap();
        reorg.apply_block_finalization_checked(&block).unwrap();
        assert_eq!(
            reorg.state_root, produced.state_root,
            "reorg restores and rederives one canonical monetary outcome"
        );
        let record = &produced.node_settlement.as_ref().unwrap().epochs[&1];
        assert_eq!(
            record.provisional.len(),
            2,
            "one-host outage must not stall surviving ws and N5"
        );
        assert!(!record.provisional.contains_key(&[3; 32]));
        assert_eq!(
            record.status,
            if boundary == close {
                EpochStatus::Provisional
            } else {
                EpochStatus::Paid
            },
            "production fold closes and matures reserve"
        );
        if boundary == maturity {
            assert_eq!(
                record.payouts.len(),
                2,
                "one-host outage still pays both serviced survivors"
            );
            let amounts: Vec<_> = record.payouts.values().copied().collect();
            assert!(amounts[0].abs_diff(amounts[1]) <= 1);
            let total: u128 = amounts.iter().sum();
            assert_eq!(total, record.reserved);
            let before_bytes = bincode::serialize(&snapshot).unwrap();
            snapshot.apply_block_finalization_checked(&block).unwrap();
            assert_eq!(
                bincode::serialize(&snapshot).unwrap(),
                before_bytes,
                "restart/snapshot cannot pay the same epoch twice"
            );
        }
        roots.insert(boundary.to_string(), hex::encode(produced.state_root.0));
        state = produced;
    }
    assert_eq!(roots.len(), 3, "ON fixture cannot be empty");
    println!(
        "NODE_SHARE_MATRIX_ROOTS {}",
        serde_json::to_string(&roots).unwrap()
    );
}

}
