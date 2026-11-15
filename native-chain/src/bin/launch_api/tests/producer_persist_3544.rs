//! Real producer persistence boundary: API, P2P, WebSocket and retry controls.
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::routes::mining::{
        MineBlockRequest, ProducerPersistTestControl, mine_block_with_persist_control,
    };
    use actix_web::HttpRequest;

    struct ErrorLog(Arc<std::sync::Mutex<Vec<String>>>);
    impl log::Log for ErrorLog {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() == log::Level::Error
        }
        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                self.0.lock().unwrap().push(record.args().to_string());
            }
        }
        fn flush(&self) {}
    }

    const MINER: &str = "0x2222222222222222222222222222222222222222";

    fn isolated(name: &str) -> bool {
        crate::helpers::test_env::isolated_chain_domain(
            name,
            "wrfcoin-main",
            "mainnet",
            "0x2727272727272727272727272727272727272727",
            "none",
        )
    }

    async fn fixture() -> (
        web::Data<AppState>,
        tempfile::TempDir,
        tokio::sync::mpsc::Receiver<wrfcoin_p2p::P2pCommand>,
    ) {
        let (p2p, commands) = P2pClient::for_testing_with_commands(libp2p::PeerId::random());
        let base = test_state_with_p2p(None, p2p);
        let dir = tempfile::tempdir().unwrap();
        let mut state = base.get_ref().clone();
        state.storage = Arc::new(RwLock::new(
            Storage::open(&dir.path().join("chain.redb"), 1).unwrap(),
        ));
        state
            .blockchain
            .write()
            .await
            .state
            .recalculate_state_root();
        state
            .storage
            .read()
            .await
            .persist_genesis_state(&state.blockchain.read().await.state)
            .await
            .unwrap();
        (web::Data::new(state), dir, commands)
    }

    fn request() -> HttpRequest {
        test::TestRequest::post()
            .uri("/blocks/mine")
            .insert_header(("Authorization", ["Bearer ", TEST_OPERATOR_TOKEN].concat()))
            .to_http_request()
    }

    fn control(omit_checkpoint: bool) -> ProducerPersistTestControl {
        ProducerPersistTestControl {
            omit_checkpoint,
            barrier: None,
            candidate_bytes: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn producer_persist_failure_keeps_every_byte_and_publishes_nothing_3544() {
        if isolated(
            "tests::producer_persist_3544::tests::producer_persist_failure_keeps_every_byte_and_publishes_nothing_3544",
        ) {
            return;
        }
        let logs = Arc::new(std::sync::Mutex::new(Vec::new()));
        log::set_boxed_logger(Box::new(ErrorLog(logs.clone()))).unwrap();
        log::set_max_level(log::LevelFilter::Error);
        let (state, _dir, mut commands) = fixture().await;
        state
            .mempool
            .write()
            .await
            .add_transaction(test_mempool_tx(1))
            .unwrap();
        state
            .mempool
            .write()
            .await
            .add_transaction(test_mempool_tx(2))
            .unwrap();
        let before = state.blockchain.read().await.clone();
        let before_bytes = state
            .storage
            .read()
            .await
            .checkpoint_state_bytes(1, &before.state)
            .unwrap()
            .unwrap();
        let pool_before = state.mempool.read().await.transaction_ids();
        let mut events = state.event_tx.subscribe();
        let fault = control(true);
        let response = mine_block_with_persist_control(
            request(),
            state.clone(),
            web::Json(MineBlockRequest {
                miner_address: MINER.into(),
            }),
            Some(&fault),
        )
        .await;
        assert!(
            commands.try_recv().is_err(),
            "failed persistence advertised the producer candidate"
        );
        assert!(
            events.try_recv().is_err(),
            "failed persistence emitted a WebSocket event"
        );
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage failure must reach the mining caller"
        );
        let body = actix_web::body::to_bytes(response.into_body())
            .await
            .unwrap();
        let envelope: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(envelope["error"]["code"], "STORAGE_ERROR");
        assert!(envelope["error"]["correlation_id"].as_str().is_some());
        assert!(
            logs.lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("producer persistence failed height=1")),
            "storage failure needs a contextual error diagnostic"
        );
        let after = state.blockchain.read().await;
        assert_eq!(after.height, before.height);
        assert_eq!(after.tip, before.tip);
        assert_eq!(after.state.state_root, before.state.state_root);
        assert_eq!(
            state
                .storage
                .read()
                .await
                .checkpoint_state_bytes(1, &after.state)
                .unwrap()
                .unwrap(),
            before_bytes
        );
        assert_eq!(after.total_block_count(), before.total_block_count());
        assert_eq!(
            after.in_memory_block_count(),
            before.in_memory_block_count()
        );
        assert_eq!(after.finalized_height(), before.finalized_height());
        drop(after);
        assert_eq!(state.mempool.read().await.transaction_ids(), pool_before);
        let durable = state.storage.read().await.load_state().unwrap();
        assert_eq!(
            durable.map(|c| c.height).unwrap_or(0),
            before.height,
            "failed checkpoint partially committed the block/WAL"
        );
        assert!(
            state
                .storage
                .read()
                .await
                .load_blocks_range(1, 1)
                .unwrap()
                .is_empty()
        );
        let app = test::init_service(test_app(state.clone())).await;
        let retry = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", ["Bearer ", TEST_OPERATOR_TOKEN].concat()))
                .set_json(json!({"miner_address":MINER}))
                .to_request(),
        )
        .await;
        assert_eq!(
            retry.status(),
            StatusCode::CREATED,
            "the next real /blocks/mine request must succeed"
        );
        let chain = state.blockchain.read().await;
        let durable = state.storage.read().await.load_state().unwrap().unwrap();
        assert_eq!(chain.height, before.height + 1);
        assert_eq!(durable.tip, chain.tip);
        assert_eq!(durable.state.state_root, chain.state.state_root);
        assert!(commands.try_recv().is_ok());
        assert!(matches!(
            events.try_recv().unwrap(),
            websocket::WsEvent::NewBlock { .. }
        ));
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn producer_publication_waits_for_durable_commit_3544() {
        if isolated(
            "tests::producer_persist_3544::tests::producer_publication_waits_for_durable_commit_3544",
        ) {
            return;
        }
        let (state, _dir, mut commands) = fixture().await;
        let mut events = state.event_tx.subscribe();
        let reached = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        let app = test::init_service(test_app(state.clone())).await;
        let ctrl = ProducerPersistTestControl {
            barrier: Some((reached.clone(), resume.clone())),
            ..control(false)
        };
        let mining = mine_block_with_persist_control(
            request(),
            state.clone(),
            web::Json(MineBlockRequest {
                miner_address: MINER.into(),
            }),
            Some(&ctrl),
        );
        let observer = async {
            tokio::time::timeout(std::time::Duration::from_secs(30), reached.notified())
                .await
                .unwrap();
            let premature_advertisement = commands.try_recv().is_ok();
            let premature_event = events.try_recv().is_ok();
            let premature_api_tip = tokio::time::timeout(
                std::time::Duration::from_millis(30),
                test::call_service(
                    &app,
                    test::TestRequest::get().uri("/blockchain").to_request(),
                ),
            )
            .await
            .is_ok();
            let durable_height = state
                .storage
                .read()
                .await
                .load_state()
                .unwrap()
                .map(|c| c.height)
                .unwrap_or(0);
            resume.notify_one();
            (
                premature_advertisement,
                premature_event,
                premature_api_tip,
                durable_height,
            )
        };
        let (response, observed) = tokio::join!(mining, observer);
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            observed,
            (false, false, false, 0),
            "no publication is permitted before persistence"
        );
        let durable = state.storage.read().await.load_state().unwrap().unwrap();
        assert_eq!(durable.height, 1);
        assert!(
            commands.try_recv().is_ok(),
            "advertisement must follow durable commit"
        );
        assert!(matches!(
            events.try_recv().unwrap(),
            websocket::WsEvent::NewBlock { height: 1, .. }
        ));
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn producer_candidate_and_published_block_bytes_are_identical_3544() {
        if isolated(
            "tests::producer_persist_3544::tests::producer_candidate_and_published_block_bytes_are_identical_3544",
        ) {
            return;
        }
        let (state, _dir, mut commands) = fixture().await;
        let ctrl = control(false);
        let response = mine_block_with_persist_control(
            request(),
            state.clone(),
            web::Json(MineBlockRequest {
                miner_address: MINER.into(),
            }),
            Some(&ctrl),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let candidate = ctrl.candidate_bytes.lock().unwrap().clone();
        assert!(!candidate.is_empty());
        let block = state
            .blockchain
            .read()
            .await
            .blocks()
            .get(&1)
            .unwrap()
            .clone();
        assert_eq!(candidate, bincode::serialize(&block).unwrap());
        let stored = state.storage.read().await.load_blocks_range(1, 1).unwrap();
        // Decoding an FxHashMap can change its bucket iteration order. Compare the
        // recovered object semantically; the storage unit asserts the raw DB bytes.
        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            serde_json::to_value(&stored[0]).unwrap()
        );
        match commands.try_recv().unwrap() {
            wrfcoin_p2p::P2pCommand::BroadcastBlock(bytes) => assert_eq!(bytes, candidate),
            _ => panic!("expected the actual block advertisement"),
        }
    }
}
