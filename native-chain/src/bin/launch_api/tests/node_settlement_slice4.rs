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
    async fn close_and_maturity_producer_peer_extractor_wal_checkpoint_and_reorg_parity() {
        const NAME: &str = "tests::node_settlement_slice4::tests::close_and_maturity_producer_peer_extractor_wal_checkpoint_and_reorg_parity";
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
                changed
                    .node_settlement
                    .as_mut()
                    .unwrap()
                    .epochs
                    .get_mut(&1)
                    .unwrap()
                    .reserved += 1;
                assert_ne!(changed.state_root_material(), peer.state_root_material());
                assert_eq!(
                    crate::p2p_events::diff_states(&peer, &changed),
                    vec!["node_settlement"],
                    "populated reserve must be visible to diff getter"
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
            assert_eq!(restored.emission_ledger.cached_supply(), Some(produced.total_minted_supply()));
            assert_eq!(restored.emission_ledger.reserved(EmissionRole::Node), produced.emission_ledger.reserved(EmissionRole::Node));
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
            assert_eq!(
                record.status,
                if boundary == close {
                    EpochStatus::Provisional
                } else {
                    EpochStatus::Paid
                }
            );
            state = produced;
        }
    }

    #[actix_web::test]
    async fn reader_records_distinguish_unknown_empty_provisional_paid() {
        const NAME: &str = "tests::node_settlement_slice4::tests::reader_records_distinguish_unknown_empty_provisional_paid";
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
        let mut state = serviced();
        server.blockchain.write().await.state = state.clone();
        let app = test::init_service(test_app(server.clone())).await;
        for (epoch, status) in [(0, "UNISSUED"), (1, "OPEN"), (999, "UNKNOWN")] {
            let response = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(&format!("/node/settlement/{epoch}"))
                    .insert_header(("Authorization", "Bearer test-reader-token"))
                    .to_request(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body: serde_json::Value = test::read_body_json(response).await;
            assert_eq!(body["status"], status);
            if status == "UNKNOWN" {
                assert!(body["unissued_base_units"].is_null());
            } else {
                assert!(body["unissued_base_units"].is_string());
            }
            if status == "UNISSUED" {
                assert_eq!(body["unissued_base_units"], body["scheduled_base_units"]);
                assert_eq!(body["outstanding_base_units"], "0");
            }
        }
        let unauthorized = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/node/settlement/1")
                .to_request(),
        )
        .await;
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        for (height, status) in [
            (epoch_close(f::bonds::A, 1).unwrap(), "PROVISIONAL"),
            (maturity_height(f::bonds::A, 1, "777").unwrap(), "PAID"),
        ] {
            advance(&mut state, height);
            server.blockchain.write().await.state = state.clone();
            let response = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri("/node/settlement/1")
                    .insert_header(("Authorization", "Bearer test-reader-token"))
                    .to_request(),
            )
            .await;
            let body: serde_json::Value = test::read_body_json(response).await;
            assert_eq!(body["status"], status);
            if status == "PAID" {
                assert_eq!(body["outstanding_base_units"], "0");
                assert!(
                    body["payouts_base_units"]
                        .as_object()
                        .unwrap()
                        .values()
                        .all(|v| v.is_string())
                );
            }
        }
    }

    #[actix_web::test]
    async fn settlement_resource_is_absent_when_off() {
        const NAME: &str =
            "tests::node_settlement_slice4::tests::settlement_resource_is_absent_when_off";
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
        wrfcoin_shared_protocol::crypto::chain_binding::install_feature_schedule(
            wrfcoin_shared_protocol::crypto::chain_binding::FeatureSchedule::from_entries(vec![])
                .unwrap(),
        );
        let server = test_state(None);
        let off = test::init_service(test_app(server.clone())).await;
        let response = test::call_service(
            &off,
            test::TestRequest::get()
                .uri("/node/settlement/0")
                .insert_header(("Authorization", "Bearer test-reader-token"))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(
            server
                .blockchain
                .read()
                .await
                .state
                .node_settlement
                .is_none()
        );
    }
}
