use super::*;
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;

#[actix_web::test]
async fn node_bond_s6a_authenticated_submit_and_truthful_reads() {
    const NAME: &str =
        "tests::node_bond_slice6a::node_bond_s6a_authenticated_submit_and_truthful_reads";
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
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;
    // Default OFF does not expose lifecycle submission or fabricate zero bonds.
    let path = format!("/node/bonds/{}", hex::encode([1; 32]));
    let response = test::call_service(&app, test::TestRequest::get().uri(&path).to_request()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let mut bonded = fixture::state();
    let claim = fixture::claim(&bonded, 1);
    let receipt_id =
        wrfcoin_shared_protocol::consensus::node_receipt_registry::claim_hash(&claim).unwrap();
    // Register authentic canonical inclusion, not a local HTTP acknowledgement.
    fixture::apply(&mut bonded, fixture::submission(vec![claim], 1)).unwrap();
    {
        let mut chain = state.blockchain.write().await;
        chain.state = bonded;
        chain.height = fixture::H;
    }
    let response = test::call_service(&app, test::TestRequest::get().uri(&path).to_request()).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&path)
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer, never a deployed credential
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["principal_base_units"], "25000000000");
    let unknown_path = format!("/node/bonds/{}", hex::encode([88; 32]));
    let response = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&unknown_path)
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .to_request(),
    )
    .await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["status"], "UNKNOWN");
    assert!(body["principal_base_units"].is_null());
    let receipt_path = format!("/node/receipts/{}", hex::encode(receipt_id));
    let response = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&receipt_path)
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .to_request(),
    )
    .await;
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["included"], true);
    assert_eq!(body["status"], "INCLUDED");
    use wrfcoin_shared_protocol::blockchain::transaction::{
        Transaction, TransactionInput, TransactionOutput, TransactionType,
    };
    let mut p = fixture::bonds::auth(
        10,
        1,
        2,
        wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAction::Exit,
        2,
    );
    let a = &p.signed.authorization;
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"synthetic-s6-api-input"),
            vout: 0,
            signature: None,
            public_key: Some(hex::encode(p.owner_key)),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            address: a.owner_account.to_string(),
            amount: 0,
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::NodeBondLifecycle;
    tx.sender_address = a.owner_account;
    tx.nonce = a.expected_nonce;
    tx.fee = 1_000_000;
    tx.data = Some(serde_json::to_string(&p).unwrap());
    tx.set_id();
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/node/bonds/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(&tx)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    use wrfcoin_shared_protocol::{
        config::settings::Config, consensus::node_bond_registry::lifecycle_verify_attempts,
        mempool::pool::Mempool,
    };
    let attempts = lifecycle_verify_attempts();
    let mut fee_config = Config::default();
    fee_config.mempool_verify_fee_per_unit = 1;
    *state.mempool.write().await = Mempool::new(&fee_config);
    let mut underfee = tx.clone();
    underfee.fee = 0;
    underfee.set_id();
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/node/bonds/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(&underfee)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        lifecycle_verify_attempts(),
        attempts,
        "HTTP underfee must do zero lifecycle verifications"
    );
    let mut size_config = Config::default();
    size_config.max_transaction_size = tx.size() - 1;
    *state.mempool.write().await = Mempool::new(&size_config);
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/node/bonds/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(&tx)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        lifecycle_verify_attempts(),
        attempts,
        "HTTP oversize must do zero lifecycle verifications"
    );
    *state.mempool.write().await = Mempool::new(&fee_config);
    let mut wrong_kind = tx.clone();
    wrong_kind.tx_type = TransactionType::Transfer;
    wrong_kind.set_id();
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/node/bonds/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(&wrong_kind)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(lifecycle_verify_attempts(), attempts);
    // The canonical height, rather than any peer-supplied value, gates activation.
    state.blockchain.write().await.height = fixture::bonds::A - 1;
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/node/bonds/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(&tx)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(lifecycle_verify_attempts(), attempts);
    state.blockchain.write().await.height = fixture::H;
    // The authority must match the canonical registry, before signature work.
    state
        .blockchain
        .write()
        .await
        .state
        .node_bond_registry
        .as_mut()
        .unwrap()
        .genesis_hash = [8; 32];
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/node/bonds/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(&tx)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(lifecycle_verify_attempts(), attempts);
    state
        .blockchain
        .write()
        .await
        .state
        .node_bond_registry
        .as_mut()
        .unwrap()
        .genesis_hash = [7; 32];
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/node/bonds/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(&tx)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        lifecycle_verify_attempts(),
        attempts + 1,
        "HTTP positive must verify lifecycle exactly once"
    );
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["included"], false);
    p.signed.owner_signature[0] ^= 1;
    tx.data = Some(serde_json::to_string(&p).unwrap());
    tx.set_id();
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/node/bonds/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(&tx)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // The dynamic receipt lookup must not swallow the pre-existing POST route.
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/node/receipts/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(serde_json::json!({}))
            .to_request(),
    )
    .await;
    assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

/// #3588: an admitted lifecycle submit is gossiped once; a refused one never is.
#[cfg(feature = "p2p")]
#[actix_web::test]
async fn node_bond_submit_gossips_admitted_tx_once_and_never_a_refused_one() {
    const NAME: &str = "tests::node_bond_slice6a::node_bond_submit_gossips_admitted_tx_once_and_never_a_refused_one";
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
    let (client, mut commands) =
        wrfcoin_p2p::P2pClient::for_testing_with_commands(libp2p::PeerId::random());
    let state = test_state_with_p2p(None, client);
    let app = test::init_service(test_app(state.clone())).await;
    {
        let mut chain = state.blockchain.write().await;
        chain.state = fixture::state();
        chain.height = fixture::H;
    }
    use wrfcoin_shared_protocol::blockchain::transaction::{
        Transaction, TransactionInput, TransactionOutput, TransactionType,
    };
    let p = fixture::bonds::auth(
        10,
        1,
        2,
        wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAction::Exit,
        2,
    );
    let a = &p.signed.authorization;
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"synthetic-3588-gossip-input"),
            vout: 0,
            signature: None,
            public_key: Some(hex::encode(p.owner_key)),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            address: a.owner_account.to_string(),
            amount: 0,
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::NodeBondLifecycle;
    tx.sender_address = a.owner_account;
    tx.nonce = a.expected_nonce;
    tx.fee = 1_000_000;
    tx.data = Some(serde_json::to_string(&p).unwrap());
    tx.set_id();
    let submit = |tx: Transaction| {
        test::TestRequest::post()
            .uri("/node/bonds/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
            .set_json(tx)
            .to_request()
    };
    // Refused: wrong tx kind, then a forged owner signature. Neither gossips.
    let mut wrong_kind = tx.clone();
    wrong_kind.tx_type = TransactionType::Transfer;
    wrong_kind.set_id();
    let response = test::call_service(&app, submit(wrong_kind)).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let mut forged = p.clone();
    forged.signed.owner_signature[0] ^= 1;
    let mut forged_tx = tx.clone();
    forged_tx.data = Some(serde_json::to_string(&forged).unwrap());
    forged_tx.set_id();
    let response = test::call_service(&app, submit(forged_tx)).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(commands.try_recv().is_err(), "refused lifecycle submit must not gossip");
    // Admitted: exactly one BroadcastTransactions carrying the tx's bincode bytes.
    let response = test::call_service(&app, submit(tx.clone())).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    match commands.try_recv() {
        Ok(wrfcoin_p2p::P2pCommand::BroadcastTransactions(batch)) => {
            assert_eq!(batch, vec![bincode::serialize(&tx).unwrap()]);
        }
        other => panic!("expected one BroadcastTransactions, got {other:?}"),
    }
    assert!(commands.try_recv().is_err(), "admitted lifecycle submit must gossip exactly once");
}
