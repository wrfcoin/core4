//! `POST /node/fraud/submit` (core4-archive#3589): flag, auth, rate limit, admission + gossip,
//! producer/applier parity for a block carrying a route-admitted report, and RED controls.
use super::*;
use wrfcoin_shared_protocol::blockchain::state::State;
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod f;
use wrfcoin_shared_protocol::consensus::{
    node_bond_registry::epoch_close, node_share_contracts::NodeBondAction,
};

#[cfg(test)]
mod tests {
    use super::*;

    const URI: &str = "/node/fraud/submit";

    /// Sets the route switch for the test's lifetime (callers hold `test_env::lock()`).
    struct Switch;
    impl Switch {
        fn on() -> Self {
            std::env::set_var(crate::routes::node_fraud::ENABLE_ENV, "1");
            crate::routes::node_fraud::reset_limiter_for_test();
            Switch
        }
    }
    impl Drop for Switch {
        fn drop(&mut self) {
            std::env::remove_var(crate::routes::node_fraud::ENABLE_ENV);
        }
    }

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
        }
    }
    fn serviced() -> State {
        let mut state = f::bonds::state();
        for (owner, id) in [(10, 1), (20, 2)] {
            f::bonds::apply(&mut state, f::bonds::auth(owner, id, 0, NodeBondAction::Deposit, 1), f::bonds::A)
                .unwrap();
        }
        advance(&mut state, f::H - 1);
        state
            .apply_transition(StateTransition::BeginNodeServiceBlock { block_height: f::H, parent_hash: [4; 32] })
            .unwrap();
        let claim = f::claim(&state, 1);
        f::apply(&mut state, f::submission(vec![claim], 1)).unwrap();
        state.apply_block_finalization(f::H);
        state
    }
    /// `forged`: the consumer receipt signature is corrupted (provable server fraud). Otherwise honest.
    fn report_tx(state: &State, forged: bool, seed: u8, nonce: u64) -> Transaction {
        let mut claim = f::claim(state, seed);
        if forged {
            claim.receipt.signature[0] ^= 1;
            f::resign_outer(&mut claim, 11, 22);
        }
        let mut p = f::submission(vec![claim], nonce);
        p.authorization.batch_hash = wrfcoin_shared_protocol::consensus::node_fraud::evidence_hash(&p).unwrap();
        p.owner_signature = f::sign(&p.authorization, 10);
        let mut tx = f::transaction(&p);
        tx.tx_type = TransactionType::NodeFraudReport;
        tx.set_id();
        tx
    }
    fn post(tx: &Transaction, token: Option<&str>) -> actix_web::test::TestRequest {
        let mut r = test::TestRequest::post().uri(URI);
        if let Some(t) = token {
            r = r.insert_header(("Authorization", format!("Bearer {t}"))); // secret-leak-ok: synthetic test bearer only.
        }
        r.set_json(tx)
    }
    async fn at_close_minus_one(server: &web::Data<AppState>) -> State {
        let close = epoch_close(f::bonds::A, 1).unwrap();
        let mut state = serviced();
        advance(&mut state, close - 1);
        let mut chain = server.blockchain.write().await;
        chain.state = state.clone();
        chain.height = close - 1;
        chain.tip = Hash::from_bytes([4; 32]);
        state
    }

    #[actix_web::test]
    async fn flag_off_is_404_before_json_extraction_and_auth() {
        const NAME: &str = "tests::node_fraud_route::tests::flag_off_is_404_before_json_extraction_and_auth";
        if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") {
            return;
        }
        let _lock = crate::helpers::test_env::lock();
        std::env::remove_var(crate::routes::node_fraud::ENABLE_ENV);
        let server = test_state(None);
        // Feature schedule active (as in the token test): only the switch can cause the 404.
        let _ = at_close_minus_one(&server).await;
        let app = test::init_service(test_app(server.clone())).await;
        // Valid operator token, invalid JSON, switch off: the route does not exist.
        let r = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(URI)
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: synthetic test bearer only.
                .insert_header(("Content-Type", "application/json"))
                .set_payload("deliberately invalid JSON")
                .to_request(),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert!(server.mempool.read().await.get_all_transactions().is_empty());
    }

    #[actix_web::test]
    async fn token_gate_and_rate_limit_30_per_minute_per_token() {
        const NAME: &str = "tests::node_fraud_route::tests::token_gate_and_rate_limit_30_per_minute_per_token";
        if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") {
            return;
        }
        let _lock = crate::helpers::test_env::lock();
        let _on = Switch::on();
        let server = test_state(None);
        // The fixture installs the NodeShareSettlement schedule the route guard needs.
        let _ = at_close_minus_one(&server).await;
        let app = test::init_service(test_app(server.clone())).await;
        let mut junk = Transaction::new(vec![], vec![]);
        junk.set_id();
        let none = test::call_service(&app, post(&junk, None).to_request()).await;
        assert_eq!(none.status(), StatusCode::UNAUTHORIZED);
        let wrong = test::call_service(&app, post(&junk, Some("not-a-token")).to_request()).await;
        assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
        let reader = test::call_service(&app, post(&junk, Some(TEST_READER_TOKEN)).to_request()).await;
        assert_eq!(reader.status(), StatusCode::FORBIDDEN);
        // Rejected callers never consume the operator's budget: 30 pass the limiter, the 31st is 429.
        let mut statuses = Vec::new();
        for _ in 0..31 {
            statuses.push(test::call_service(&app, post(&junk, Some(TEST_OPERATOR_TOKEN)).to_request()).await.status());
        }
        assert!(statuses[..30].iter().all(|s| *s != StatusCode::TOO_MANY_REQUESTS), "{statuses:?}");
        assert_eq!(statuses[30], StatusCode::TOO_MANY_REQUESTS);
        // RED control: with the switch on and budget spent, a different token is unaffected.
        let other = test::call_service(&app, post(&junk, Some(TEST_ORACLE_TOKEN)).to_request()).await;
        assert_eq!(other.status(), StatusCode::FORBIDDEN, "non-operator never reaches the limiter");
    }

    #[actix_web::test]
    async fn valid_report_is_admitted_gossiped_and_block_has_identical_state_root() {
        const NAME: &str = "tests::node_fraud_route::tests::valid_report_is_admitted_gossiped_and_block_has_identical_state_root";
        if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") {
            return;
        }
        let _lock = crate::helpers::test_env::lock();
        let _on = Switch::on();
        #[cfg(feature = "p2p")]
        let (client, mut gossip_rx) = wrfcoin_p2p::P2pClient::for_testing_with_commands(libp2p::PeerId::random());
        #[cfg(feature = "p2p")]
        let server = test_state_with_p2p(None, client);
        #[cfg(not(feature = "p2p"))]
        let server = test_state(None);
        let state = at_close_minus_one(&server).await;
        let app = test::init_service(test_app(server.clone())).await;

        // RED control: an honest (non-fraudulent) claim is authorized but not provable: 422, not admitted, not gossiped.
        let honest = report_tx(&state, false, 9, 2);
        let red = test::call_service(&app, post(&honest, Some(TEST_OPERATOR_TOKEN)).to_request()).await;
        assert_eq!(red.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value = test::read_body_json(red).await;
        assert!(body.to_string().contains("NODE_FRAUD_UNPROVABLE"), "{body}");
        assert!(server.mempool.read().await.get_all_transactions().is_empty());
        #[cfg(feature = "p2p")]
        assert!(gossip_rx.try_recv().is_err(), "refused report must not gossip");

        // Wrong tx type is a payload error, not an admission.
        let mut wrong_type = report_tx(&state, true, 9, 2);
        wrong_type.tx_type = TransactionType::NodeServiceBatch;
        wrong_type.set_id();
        let r = test::call_service(&app, post(&wrong_type, Some(TEST_OPERATOR_TOKEN)).to_request()).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);

        // A genuinely forged consumer receipt is admitted and gossiped exactly once.
        let tx = report_tx(&state, true, 9, 2);
        let ok = test::call_service(&app, post(&tx, Some(TEST_OPERATOR_TOKEN)).to_request()).await;
        let status = ok.status();
        let body: serde_json::Value = test::read_body_json(ok).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["admitted"], true);
        assert_eq!(body["responsible"].as_array().unwrap().len(), 1);
        assert_eq!(server.mempool.read().await.get_all_transactions().len(), 1);
        #[cfg(feature = "p2p")]
        {
            match gossip_rx.try_recv() {
                Ok(wrfcoin_p2p::P2pCommand::BroadcastTransactions(batch)) => {
                    assert_eq!(batch, vec![bincode::serialize(&tx).unwrap()]);
                },
                other => panic!("expected one BroadcastTransactions, got {other:?}"),
            }
            assert!(gossip_rx.try_recv().is_err(), "admitted report must gossip exactly once");
        }

        // Producer, extractor (applier/WAL path) derive the same state_root for the block carrying it.
        let before = server.blockchain.read().await.clone();
        let block = server
            .consensus
            .create_pop_block_with_state(server.blockchain.clone(), server.mempool.clone(), &f::bonds::address(10).to_string())
            .await
            .unwrap();
        assert_eq!(
            block.transactions.iter().filter(|t| t.tx_type == TransactionType::NodeFraudReport).count(),
            1,
            "route-admitted report is included"
        );
        let produced = server.blockchain.read().await.state.clone();
        assert!(produced.node_fraud.as_ref().unwrap().jailed.contains(&[1; 32]));
        let transitions = crate::transition_extractor::extract_transitions(&before.state, &block, block.height, block.timestamp.timestamp());
        let mut extracted = before.state.clone();
        extracted.apply_transitions_batch_at_height(block.height, transitions).unwrap();
        extracted.apply_block_finalization_checked(&block).unwrap();
        assert_eq!(produced.state_root, extracted.state_root, "producer and applier state_root");
        assert_eq!(produced.node_fraud, extracted.node_fraud);
        let transitions = crate::transition_extractor::extract_transitions(&before.state, &block, block.height, block.timestamp.timestamp());
        server.storage.write().await.persist_block(block.clone(), transitions, None).await.unwrap();
        server.storage.write().await.force_checkpoint(&produced, block.height).await.unwrap();
        let restored = server.storage.read().await.load_state().unwrap().unwrap().state;
        assert_eq!(restored.state_root, produced.state_root, "persisted WAL/checkpoint state_root");

        // Once sanctioned, the same evidence from another nonce is refused at the edge with 409.
        let again = report_tx(&produced, true, 9, 3);
        let dup = test::call_service(&app, post(&again, Some(TEST_OPERATOR_TOKEN)).to_request()).await;
        assert_eq!(dup.status(), StatusCode::CONFLICT);
    }
}
