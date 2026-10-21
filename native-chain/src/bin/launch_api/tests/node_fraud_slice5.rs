use super::*;
use wrfcoin_shared_protocol::blockchain::state::State;
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod f;
use wrfcoin_shared_protocol::consensus::{
    node_bond_registry::epoch_close,
    node_settlement::{EpochStatus, maturity_height},
    node_share_contracts::NodeBondAction,
    proof_of_publishing::EmissionRole,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn advance(state: &mut State, target: u64) {
        let start = state.node_bond_registry.as_ref().unwrap().block_height;
        state.apply_block_finalization(start);
        for h in start + 1..=target {
            state
                .apply_transition(StateTransition::BeginNodeServiceBlock {
                    block_height: h,
                    parent_hash: [(h % 251) as u8; 32],
                })
                .unwrap();
            state.apply_block_finalization(h);
            state.validate_node_finalized(h).unwrap();
            assert!(state.supply_violation.is_none());
        }
    }
    fn serviced() -> State {
        let mut state = f::bonds::state();
        for (owner, id) in [(10, 1), (20, 2)] {
            f::bonds::apply(
                &mut state,
                f::bonds::auth(owner, id, 0, NodeBondAction::Deposit, 1),
                f::bonds::A,
            )
            .unwrap();
        }
        advance(&mut state, f::H - 1);
        state
            .apply_transition(StateTransition::BeginNodeServiceBlock {
                block_height: f::H,
                parent_hash: [4; 32],
            })
            .unwrap();
        let claim = f::claim(&state, 1);
        f::apply(&mut state, f::submission(vec![claim], 1)).unwrap();
        state.apply_block_finalization(f::H);
        state
    }

    #[actix_web::test]
    async fn confiscation_producer_peer_extractor_wal_checkpoint_and_maturity_parity() {
        const NAME: &str = "tests::node_fraud_slice5::tests::confiscation_producer_peer_extractor_wal_checkpoint_and_maturity_parity";
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
        let close = epoch_close(f::bonds::A, 1).unwrap();
        let maturity = maturity_height(f::bonds::A, 1, "777").unwrap();
        let mut state = serviced();
        for boundary in [close, maturity] {
            advance(&mut state, boundary - 1);
            {
                let mut chain = server.blockchain.write().await;
                chain.state = state.clone();
                chain.height = boundary - 1;
                chain.tip = Hash::from_bytes([4; 32]);
            }
            if boundary == close {
                let mut claim = f::claim(&state, 9);
                claim.receipt.signature[0] ^= 1;
                f::resign_outer(&mut claim, 11, 22);
                let mut p = f::submission(vec![claim], 2);
                p.authorization.batch_hash =
                    wrfcoin_shared_protocol::consensus::node_fraud::evidence_hash(&p).unwrap();
                p.owner_signature = f::sign(&p.authorization, 10);
                let mut tx = f::transaction(&p);
                tx.tx_type = TransactionType::NodeFraudReport;
                tx.set_id();
                server
                    .mempool
                    .write()
                    .await
                    .add_transaction_with_signing_keys_at_height(tx, None, Some(close))
                    .unwrap();
            }
            let before = server.blockchain.read().await.clone();
            let supply_before = before.state.total_minted_supply();
            let journal_before = before.state.supply_authorized_minting;
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
            if boundary == close {
                assert_eq!(
                    block
                        .transactions
                        .iter()
                        .filter(|t| t.tx_type == TransactionType::NodeFraudReport)
                        .count(),
                    1
                );
            }
            let sink = wrfcoin_shared_protocol::consensus::node_fraud::forfeiture_address();
            let escrow =
                wrfcoin_shared_protocol::consensus::node_bond_registry::node_bond_escrow_address(
                    &[1; 32],
                );
            assert_eq!(
                produced.accounts[&escrow].balance.0, 0,
                "real production escrow debit"
            );
            assert_eq!(
                produced.accounts[&sink].balance.0,
                u128::from(wrfcoin_shared_protocol::consensus::node_bond_registry::NODE_BOND_UNITS)
            );
            assert_eq!(
                produced.accounts[&sink].balance,
                produced.accounts[&sink].stake_locked
            );
            assert_eq!(produced.accounts[&sink].spendable_balance.0, 0);
            assert!(
                produced
                    .node_fraud
                    .as_ref()
                    .unwrap()
                    .jailed
                    .contains(&[1; 32])
            );
            assert_eq!(
                produced.total_minted_supply() - supply_before,
                produced.supply_authorized_minting - journal_before,
                "only authorized existing role rewards change supply; confiscation is a transfer"
            );
            assert_eq!(
                produced.supply_authorized_burning,
                before.state.supply_authorized_burning
            );

            let transitions = crate::transition_extractor::extract_transitions(
                &before.state,
                &block,
                block.height,
                block.timestamp.timestamp(),
            );
            let mut extracted = before.state.clone();
            extracted
                .apply_transitions_batch_at_height(block.height, transitions)
                .unwrap();
            extracted.apply_block_finalization_checked(&block).unwrap();
            assert_eq!(
                produced.state_root, extracted.state_root,
                "extractor derives boundary settlement"
            );
            assert_eq!(produced.node_settlement, extracted.node_settlement);
            assert_eq!(produced.node_fraud, extracted.node_fraud);
            assert_eq!(produced.accounts, extracted.accounts);
            let mut metadata_block = block.clone();
            metadata_block.metadata.insert(
                "node_settlement".into(),
                "mint=999999999999999999999;eligible=attacker".into(),
            );
            let mut metadata_state = before.state.clone();
            let transitions = crate::transition_extractor::extract_transitions(
                &metadata_state,
                &metadata_block,
                block.height,
                block.timestamp.timestamp(),
            );
            metadata_state
                .apply_transitions_batch_at_height(block.height, transitions)
                .unwrap();
            metadata_state
                .apply_block_finalization_checked(&metadata_block)
                .unwrap();
            assert_eq!(
                metadata_state.node_settlement, produced.node_settlement,
                "metadata has no Node authority"
            );
            assert_eq!(metadata_state.state_root, produced.state_root);
            #[cfg(feature = "p2p")]
            {
                let peer =
                    crate::p2p_events::apply_node_receipt_block_for_test(before.clone(), &block)
                        .await;
                assert_eq!(
                    peer.state_root, produced.state_root,
                    "actual peer and persisted WAL boundary parity"
                );
                assert!(crate::p2p_events::diff_states(&peer, &produced).is_empty());
                let mut changed = peer.clone();
                changed.node_fraud.as_mut().unwrap().confiscated += 1;
                assert_ne!(changed.state_root_material(), peer.state_root_material());
                assert_eq!(
                    crate::p2p_events::diff_states(&peer, &changed),
                    vec!["node_fraud"]
                );
            }
            // Persist producer checkpoint and read the real envelope back.
            server
                .storage
                .write()
                .await
                .force_checkpoint(&before.state, before.height)
                .await
                .unwrap();
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
                .persist_block(block.clone(), inputs, None)
                .await
                .unwrap();
            server
                .storage
                .write()
                .await
                .force_checkpoint(&produced, block.height)
                .await
                .unwrap();
            let restored = server
                .storage
                .read()
                .await
                .load_state()
                .unwrap()
                .unwrap()
                .state;
            assert_eq!(restored.node_settlement, produced.node_settlement);
            assert_eq!(restored.node_fraud, produced.node_fraud);
            assert_eq!(restored.accounts, produced.accounts);
            assert_eq!(
                restored.emission_ledger.cached_supply(),
                Some(produced.total_minted_supply())
            );
            assert_eq!(
                restored.emission_ledger.reserved(EmissionRole::Node),
                produced.emission_ledger.reserved(EmissionRole::Node)
            );
            assert_eq!(restored.emission_ledger.minted(EmissionRole::Node), 0);
            assert_eq!(restored.state_root, produced.state_root);
            // Fork rollback to the pre-boundary state and deterministic reapplication.
            let mut rollback = before.state.clone();
            let transitions = crate::transition_extractor::extract_transitions(
                &rollback,
                &block,
                block.height,
                block.timestamp.timestamp(),
            );
            rollback
                .apply_transitions_batch_at_height(block.height, transitions)
                .unwrap();
            rollback.apply_block_finalization_checked(&block).unwrap();
            assert_eq!(rollback.state_root, produced.state_root);
            let record = &produced.node_settlement.as_ref().unwrap().epochs[&1];
            assert_eq!(record.status, EpochStatus::Unissued);
            assert_eq!(record.unissued, record.scheduled);
            let entries =
                wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries(
                    &produced,
                )
                .unwrap();
            let snapshot =
                wrfcoin_shared_protocol::blockchain::state_snapshot::import_state_entries(
                    &State::new(),
                    &entries,
                )
                .unwrap();
            assert_eq!(snapshot.state_root, produced.state_root);
            assert_eq!(snapshot.accounts, produced.accounts);
            assert_eq!(snapshot.node_fraud, produced.node_fraud);

            state = produced;
        }
    }
}
