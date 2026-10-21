use super::*;
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
use super::*;

struct OwnedDaemon(std::process::Child);
impl Drop for OwnedDaemon {
    fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
}
fn receiver_daemon(root: &std::path::Path, port: u16, genesis: &str, sync_only: bool) -> OwnedDaemon {
    std::fs::write(root.join(".node-share-rehearsal-receiver"), genesis).unwrap();
    std::fs::create_dir(root.join("home")).unwrap();
    let log = std::fs::File::create(root.join("daemon.log")).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::node_share_rehearsal_daemon_entry", "--ignored", "--nocapture"])
        .current_dir(root).env_clear().env("HOME", root.join("home")).env("RUST_LOG", "error")
        .env("WRFCOIN_CHAIN_ID", "777").env("WRFCOIN_NETWORK_ID", "testnet")
        .env("WRFCOIN_NODE_SHARE_TEST_DAEMON", "1")
        .env("WRFCOIN_NODE_SHARE_REHEARSAL", "true")
        .env("WRFCOIN_SYNC_ONLY", if sync_only { "true" } else { "false" })
        .env("WRFCOIN_P2P_ENABLED", "false")
        .env("WRFCOIN_API_HOST", "127.0.0.1").env("WRFCOIN_API_PORT", port.to_string())
        .env("WRFCOIN_DATA_DIR", root.join("database"))
        .env("WRFCOIN_GENESIS_HASH", genesis)
        .env("WRFCOIN_STORAGE_ENCRYPTION_KEY", "66".repeat(32)) // secret-leak-ok: synthetic at-rest key for owned test daemon
        .env("WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS", hex::encode(fixture::pk(11)))
        .env("WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS", hex::encode(fixture::pk(99)))
        .env("WRFCOIN_ORACLE_GOVERNANCE_PUBKEY_COUNT", "1")
        .env("WRFCOIN_OPERATOR_TOKEN", TEST_OPERATOR_TOKEN) // secret-leak-ok: fixed synthetic principal for owned loopback test daemon
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::from(log))
        .spawn().unwrap();
    OwnedDaemon(child)
}

#[actix_web::test]
async fn actual_bonded_receiver_install_endorse_archive_submit_and_extractor_parity() {
    const NAME: &str = "tests::node_receipt_slice3::tests::actual_bonded_receiver_install_endorse_archive_submit_and_extractor_parity";
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
    use wrfcoin_shared_protocol::{
        consensus::{node_receipt_registry::*, node_share_contracts::*},
        crypto::chain_binding::{FeatureSchedule, install_feature_schedule},
    };
    install_feature_schedule(FeatureSchedule::from_entries(vec![]).unwrap());
    let headers = crate::routes::snapshot::snapshot_test_support::chain_committing_to(
        "placeholder",
        fixture::H,
    );
    let genesis = headers[0].hash.unwrap();
    let genesis_bytes = *genesis.as_bytes();
    let mut bonded = fixture::bonds::state_on(
        genesis_bytes,
        *headers[(fixture::bonds::A - 2) as usize]
            .hash
            .unwrap()
            .as_bytes(),
    );
    for (owner, id) in [(10, 1), (20, 2)] {
        fixture::bonds::apply(
            &mut bonded,
            fixture::bonds::auth_on(owner, id, 0, NodeBondAction::Deposit, 1, genesis_bytes),
            fixture::bonds::A,
        )
        .unwrap();
    }
    for h in fixture::bonds::A + 1..=fixture::H {
        bonded
            .apply_transition(StateTransition::BeginNodeServiceBlock {
                block_height: h,
                parent_hash: *headers[(h - 2) as usize].hash.unwrap().as_bytes(),
            })
            .unwrap();
        bonded.apply_block_finalization(h);
    }
    #[cfg(feature = "p2p")]
    let (gossip_client, mut gossip_rx) = wrfcoin_p2p::P2pClient::for_testing_with_commands(libp2p::PeerId::random());
    #[cfg(feature = "p2p")]
    let server = test_state_with_p2p(Some(fixture::key(11)), gossip_client);
    #[cfg(not(feature = "p2p"))]
    let server = test_state(Some(fixture::key(11)));
    let mut receiver_inner = test_state(Some(fixture::key(22))).into_inner();
    Arc::get_mut(&mut receiver_inner)
        .unwrap()
        .snapshot_trusted_signers = Arc::new([hex::encode(fixture::pk(11))].into_iter().collect());
    let receiver = web::Data::from(receiver_inner);
    {
        let mut chain = server.blockchain.write().await;
        chain.state = bonded;
        *chain = crate::routes::snapshot::snapshot_test_support::with_chain_committing_to_its_state(
            std::mem::take(&mut *chain),
            fixture::H,
        );
        chain.set_finalized_height(fixture::H);
    }
    let _pin = crate::routes::snapshot::TestGenesisPin::set(genesis.to_hex());
    let server_app = test::init_service(test_app(server.clone())).await;
    let receiver_app = test::init_service(test_app(receiver.clone())).await;
    let served: SnapshotSyncPayloadResponse = test::read_body_json(
        test::call_service(
            &server_app,
            test::TestRequest::get()
                .uri("/state/snapshot/sync-payload")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed test bearer defined in main.rs; never a deployed credential.
                .to_request(),
        )
        .await,
    )
    .await; // secret-leak-ok: synthetic test bearer only.
    let signed = served.signed_payload.unwrap();
    let page: crate::routes::snapshot::SnapshotExportResponse = test::read_body_json(
        test::call_service(
            &server_app,
            test::TestRequest::get()
                .uri("/state/snapshot/export?limit=10000")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed test bearer defined in main.rs; never a deployed credential.
                .to_request(),
        )
        .await,
    )
    .await; // secret-leak-ok: synthetic test bearer only.
    assert!(page.next_offset.is_none(), "fixture fits one export page");
    // Real daemon entry calls main(), including role resolution, signer loading
    // and consensus production disablement. Only its hardware backend is synthetic.
    let refused_root = tempfile::tempdir().unwrap();
    let refused_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let refused_port = refused_listener.local_addr().unwrap().port(); drop(refused_listener);
    let mut refused_daemon = receiver_daemon(refused_root.path(), refused_port, &genesis.to_hex(), true);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = refused_daemon.0.try_wait().unwrap() {
            assert!(!status.success()); break;
        }
        assert!(std::time::Instant::now() < deadline, "sync-only rehearsal must refuse startup");
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(std::fs::read_to_string(refused_root.path().join("daemon.log")).unwrap()
        .contains("signing rehearsal refuses sync-only posture"));
    assert!(!refused_root.path().join("database").exists());
    let daemon_root = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port(); drop(listener);
    let mut daemon = receiver_daemon(daemon_root.path(), port, &genesis.to_hex(), false);
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        assert!(daemon.0.try_wait().unwrap().is_none(), "receiver startup failed: {}",
            std::fs::read_to_string(daemon_root.path().join("daemon.log")).unwrap());
        if let Ok(response) = client.get(format!("{base}/node/sync/status"))
            .bearer_auth(TEST_OPERATOR_TOKEN).send().await { // secret-leak-ok: fixed synthetic test principal
            if response.status().is_success() {
                let status: serde_json::Value = response.json().await.unwrap();
                assert_eq!(status["chain_height"],0);assert_eq!(status["blocks_in_memory"],0);break;
            }
        }
        assert!(std::time::Instant::now() < deadline, "receiver startup timed out");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let installed_response = client.post(format!("{base}/network/snapshot-sync/receiver-ingest"))
        .bearer_auth(TEST_OPERATOR_TOKEN) // secret-leak-ok: fixed synthetic test principal
        .json(&json!({"signed_payload":signed,"entries":page.entries,"served_by":"node-1"}))
        .send().await.unwrap();
    let installed_status = installed_response.status();
    let actual_installed: serde_json::Value = installed_response.json().await.unwrap();
    #[cfg(not(feature = "p2p"))]
    {
        // A build without the pinned-genesis provider must refuse ingestion.
        assert_eq!(installed_status.as_u16(), 422);
        assert_eq!(actual_installed["error"]["code"], "NODE_SERVICE_INACTIVE");
        assert!(actual_installed.get("receipt").is_none());
    }
    #[cfg(feature = "p2p")]
    {
    assert!(installed_status.is_success(), "real daemon install: {actual_installed}");
    assert_eq!(actual_installed["applied"],true);
    let actual_archive = wrfcoin_native_chain::node_service_archive::InstalledNodeReceipt {
        receipt: serde_json::from_value(actual_installed["receipt"].clone()).unwrap(), manifest: signed.clone(),
    };
    assert_eq!(actual_archive.receipt.receipt.consumer_pubkey,fixture::pk(22));
    // The in-process server needs its canonical ON view; the actual receiver
    // has already derived its own overlay in a separate daemon process.
    wrfcoin_shared_protocol::crypto::chain_binding::refresh_feature_schedule_extensions(
        server.blockchain.read().await.state.feature_schedule_extensions.iter()
            .flat_map(|extension| extension.entries.iter())
            .map(|entry| (entry.feature.clone(), entry.activation_height)).collect());
    let actual_ticket = test::call_service(&server_app,test::TestRequest::post()
        .uri("/network/snapshot-sync/bonded-ticket")
        .insert_header(("Authorization",format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test principal
        .set_json(&actual_archive).to_request()).await;
    assert_eq!(actual_ticket.status(),StatusCode::OK);
    let actual_ticket: serde_json::Value = test::read_body_json(actual_ticket).await;
    let actual_endorsement = client.post(format!("{base}/network/snapshot-sync/bonded-endorsement"))
        .bearer_auth(TEST_OPERATOR_TOKEN).json(&actual_ticket).send().await.unwrap(); // secret-leak-ok: fixed synthetic test principal
    let actual_endorsement_status = actual_endorsement.status();
    let actual_endorsement: serde_json::Value = actual_endorsement.json().await.unwrap();
    assert!(actual_endorsement_status.is_success(), "real daemon endorsement: {actual_endorsement}");
    let actual_claim = BondedServiceClaim::from_wire(&hex::decode(actual_endorsement["claim_hex"].as_str().unwrap()).unwrap()).unwrap();
    actual_claim.verify_contract(&fixture::identity(), &genesis_bytes, &fixture::pk(11), &fixture::pk(22), wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(fixture::H)).unwrap();
    let refused_mining = client.post(format!("{base}/blocks/mine"))
        .bearer_auth(TEST_OPERATOR_TOKEN).json(&json!({"miner_address":fixture::bonds::address(20).to_string()}))
        .send().await.unwrap(); // secret-leak-ok: fixed synthetic test principal
    assert_eq!(refused_mining.status().as_u16(),503,"signing consumer must not produce");
    let refused_mining: serde_json::Value = refused_mining.json().await.unwrap();
    assert_eq!(refused_mining["error"]["code"],"SYNC_ONLY","consensus production guard must refuse the signing consumer");
    let status: serde_json::Value = client.get(format!("{base}/node/sync/status"))
        .bearer_auth(TEST_OPERATOR_TOKEN).send().await.unwrap().json().await.unwrap(); // secret-leak-ok: fixed synthetic test principal
    assert_eq!(status["chain_height"],fixture::H);
    }
    drop(daemon);

    // Real fresh-node entry: no Node activation is known before trusted import.
    wrfcoin_shared_protocol::crypto::chain_binding::refresh_feature_schedule_extensions(vec![]);
    assert!(!wrfcoin_shared_protocol::crypto::chain_binding::feature_active(wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature::NodeShareSettlement, u64::MAX));

    let installed = test::call_service(
        &receiver_app,
        test::TestRequest::post()
            .uri("/network/snapshot-sync/receiver-ingest")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed test bearer defined in main.rs; never a deployed credential.
            .set_json(json!({"signed_payload":signed,"entries":page.entries,"served_by":"node-1"}))
            .to_request(),
    )
    .await; // secret-leak-ok: synthetic test bearer only.
    let installed_status=installed.status();
    let installed: serde_json::Value = test::read_body_json(installed).await;
    assert_eq!(installed_status, StatusCode::OK, "{installed}");
    assert_eq!(installed["applied"], true);
    let archive = wrfcoin_native_chain::node_service_archive::InstalledNodeReceipt {
        receipt: serde_json::from_value(installed["receipt"].clone()).unwrap(),
        manifest: signed,
    };
    assert_eq!(archive.receipt.receipt.consumer_pubkey, fixture::pk(22));
    let ticket = test::call_service(
        &server_app,
        test::TestRequest::post()
            .uri("/network/snapshot-sync/bonded-ticket")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed test bearer defined in main.rs; never a deployed credential.
            .set_json(&archive)
            .to_request(),
    )
    .await; // secret-leak-ok: synthetic test bearer only.
    assert_eq!(ticket.status(), StatusCode::OK);
    let ticket: serde_json::Value = test::read_body_json(ticket).await;
    // A dry-run receiver shares the chain view but has no durable successful-install journal.
    let dry = test_state(Some(fixture::key(22)));
    {
        let source = receiver.blockchain.read().await;
        let mut dry_chain = dry.blockchain.write().await;
        dry_chain.state = source.state.clone();
        dry_chain.height = source.height;
        dry_chain.tip = source.tip;
    }
    let dry_app = test::init_service(test_app(dry)).await;
    let refused = test::call_service(
        &dry_app,
        test::TestRequest::post()
            .uri("/network/snapshot-sync/bonded-endorsement")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed test bearer defined in main.rs; never a deployed credential.
            .set_json(&ticket)
            .to_request(),
    )
    .await; // secret-leak-ok: synthetic test bearer only.
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let refused: serde_json::Value = test::read_body_json(refused).await;
    assert_eq!(refused["error"]["code"], "NODE_RECEIPT_NOT_INSTALLED");
    let endorsed = test::call_service(
        &receiver_app,
        test::TestRequest::post()
            .uri("/network/snapshot-sync/bonded-endorsement")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed test bearer defined in main.rs; never a deployed credential.
            .set_json(ticket)
            .to_request(),
    )
    .await; // secret-leak-ok: synthetic test bearer only.
    assert_eq!(endorsed.status(), StatusCode::OK, "durable archive must authorize actual installed receiver endorsement");
    let endorsed: serde_json::Value = test::read_body_json(endorsed).await;
    let claim = BondedServiceClaim::from_wire(
        &hex::decode(endorsed["claim_hex"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    let receipt_key = BondedServiceClaim::receipt_hash(&archive.receipt).unwrap();
    assert!(
        receiver
            .storage
            .read()
            .await
            .archived_node_receipt(&receipt_key)
            .unwrap()
            .is_some()
    );
    let batch = NodeServiceBatch {
        claims: vec![claim],
    };
    let authorization = NodeReceiptSubmissionAuthorization {
        genesis_hash: genesis_bytes,
        sender: fixture::bonds::address(10),
        nonce: 1,
        batch_hash: batch_hash(&batch).unwrap(),
    };
    let p = NodeReceiptSubmission {
        owner_signature: fixture::sign(&authorization, 10),
        authorization,
        batch,
        owner_key: fixture::pk(10),
    };
    // Copying public service evidence does not license an unbonded wallet.
    let mut unbonded = p.clone();
    unbonded.authorization.sender = fixture::bonds::address(30);
    unbonded.authorization.nonce = 0;
    unbonded.owner_key = fixture::pk(30);
    unbonded.owner_signature = fixture::sign(&unbonded.authorization, 30);
    let unbonded_tx = fixture::transaction(&unbonded);
    let archive_key = *wrfcoin_shared_protocol::crypto::hash::hash_bytes(&serde_json::to_vec(&unbonded_tx).unwrap()).as_bytes();
    let refused = test::call_service(&server_app, test::TestRequest::post().uri("/node/receipts/submit").insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))).set_json(&unbonded_tx).to_request()).await; // secret-leak-ok: synthetic test bearer only.
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(server.storage.read().await.archived_node_receipt(&archive_key).unwrap().is_none());
    // #3588: a refused submit is never gossiped.
    #[cfg(feature = "p2p")]
    assert!(gossip_rx.try_recv().is_err(), "refused receipt submit must not gossip");
    let tx = fixture::transaction(&p);
    let admitted = test::call_service(
        &server_app,
        test::TestRequest::post()
            .uri("/node/receipts/submit")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed test bearer defined in main.rs; never a deployed credential.
            .set_json(&tx)
            .to_request(),
    )
    .await; // secret-leak-ok: synthetic test bearer only.
    let admitted_status = admitted.status();
    let admitted: serde_json::Value = test::read_body_json(admitted).await;
    assert_eq!(admitted_status, StatusCode::ACCEPTED, "{admitted}");
    assert_eq!(admitted["included"], false);
    assert_eq!(server.mempool.read().await.get_all_transactions().len(), 1);
    // #3588: the admitted submit is gossiped exactly once, as the tx's bincode bytes.
    #[cfg(feature = "p2p")]
    {
        match gossip_rx.try_recv() {
            Ok(wrfcoin_p2p::P2pCommand::BroadcastTransactions(batch)) => {
                assert_eq!(batch, vec![bincode::serialize(&tx).unwrap()]);
            }
            other => panic!("expected one BroadcastTransactions, got {other:?}"),
        }
        assert!(gossip_rx.try_recv().is_err(), "admitted receipt submit must gossip exactly once");
    }
    // #3588: the same batch arriving by gossip is admitted by a peer's pool; a forged one is not.
    #[cfg(feature = "p2p")]
    {
        use crate::p2p_events::{GossipIngest, ingest_gossiped_transaction_at_height};
        let keys = wrfcoin_native_chain::security::key_registry::SigningKeyRegistry::default();
        let mut peer_pool = wrfcoin_shared_protocol::mempool::pool::Mempool::new(&wrfcoin_shared_protocol::config::settings::Config::default());
        let mut forged = p.clone();
        forged.owner_signature[0] ^= 1;
        assert_eq!(ingest_gossiped_transaction_at_height(&mut peer_pool, fixture::transaction(&forged), &keys, Some(fixture::H)), GossipIngest::Rejected, "forged receipt batch gossip must be refused");
        assert_eq!(ingest_gossiped_transaction_at_height(&mut peer_pool, tx.clone(), &keys, Some(fixture::H)), GossipIngest::Admitted, "valid NodeServiceBatch gossip must be admitted");
    }
    let before_chain = server.blockchain.read().await.clone();
    let before = before_chain.state.clone();
    let block = server.consensus.create_pop_block_with_state(
        server.blockchain.clone(), server.mempool.clone(), &fixture::bonds::address(10).to_string(),
    ).await.expect("actual producer mines the admitted receipt");
    assert_eq!(block.height, fixture::H + 1);
    assert_eq!(block.transactions.iter().filter(|t| t.tx_type == TransactionType::NodeServiceBatch).count(), 1);
    let producer = server.blockchain.read().await.state.clone();
    // Canonical readback proves mined inclusion rather than HTTP acceptance.
    let receipt_id = hex::encode(claim_hash(&p.batch.claims[0]).unwrap());
    let readback = test::call_service(&server_app, test::TestRequest::get()
        .uri(&format!("/node/receipts/{receipt_id}"))
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic bearer for loopback integration.
        .to_request()).await;
    assert_eq!(readback.status(), StatusCode::OK);
    let readback: serde_json::Value = test::read_body_json(readback).await;
    assert_eq!(readback["receipt_id"], receipt_id);
    assert_eq!(readback["status"], "INCLUDED");
    assert_eq!(readback["included"], true);
    #[cfg(feature = "p2p")]
    {
        let peer = crate::p2p_events::apply_node_receipt_block_for_test(before_chain, &block).await;
        assert_eq!(peer.state_root, producer.state_root, "actual producer/peer receipt root parity");
        assert!(crate::p2p_events::diff_states(&producer, &peer).is_empty());
        for marker in [false, true] {
            let mut changed = peer.clone();
            if marker { changed.node_receipt_registry = None; }
            else { changed.node_receipt_registry.as_mut().unwrap().accepted.values_mut().next().unwrap().claim.receipt.receipt.units += 1; }
            assert_ne!(changed.state_root_material(), peer.state_root_material());
            assert_eq!(crate::p2p_events::diff_states(&peer, &changed), vec!["node_receipt_registry"]);
        }
    }

    let extracted = crate::transition_extractor::extract_transitions(
        &before,
        &block,
        block.height,
        block.timestamp.timestamp(),
    );
    let mut follower = before.clone();
    follower
        .apply_transitions_batch_at_height(block.height, extracted.clone())
        .unwrap();
    follower.apply_block_finalization_checked(&block).unwrap();
    assert_eq!(follower.state_root, producer.state_root, "actual producer/extractor root parity");
    assert_eq!(follower.node_receipt_registry, producer.node_receipt_registry);
    let mut direct = before;
    direct
        .apply_transitions_batch_at_height(
            block.height,
            vec![
                StateTransition::BeginNodeServiceBlock {
                    block_height: block.height,
                    parent_hash: *block.prev_block_hash.as_bytes(),
                },
                StateTransition::ApplyNodeServiceBatch {
                    payload: p,
                    block_height: block.height,
                },
            ],
        )
        .unwrap();
    direct.apply_block_finalization(block.height);
    assert_eq!(follower.node_receipt_registry, direct.node_receipt_registry);
    assert!(
        follower
            .node_receipt_registry
            .as_ref()
            .unwrap()
            .has_service(1, &[1; 32])
    );
}

#[actix_web::test]
async fn node_receipt_resources_are_absent_and_allocate_no_archive_when_off() {
    const NAME: &str = "tests::node_receipt_slice3::tests::node_receipt_resources_are_absent_and_allocate_no_archive_when_off";
    if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") { return; }
    let _lock = crate::helpers::test_env::lock();
    use wrfcoin_shared_protocol::crypto::chain_binding::{FeatureSchedule, install_feature_schedule};
    install_feature_schedule(FeatureSchedule::from_entries(vec![]).unwrap());
    let state = test_state(Some(fixture::key(22)));
    let root = state.blockchain.read().await.state.state_root_material();
    let app = test::init_service(test_app(state.clone())).await;
    for path in ["/network/snapshot-sync/bonded-ingest", "/network/snapshot-sync/bonded-ticket", "/network/snapshot-sync/bonded-endorsement", "/node/receipts/submit"] {
        let response = test::call_service(&app, test::TestRequest::post().uri(path).insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))).insert_header(("Content-Type", "application/json")).set_payload("deliberately invalid JSON").to_request()).await; // secret-leak-ok: synthetic test bearer only.
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "OFF resource {path} must refuse before JSON extraction");
    }
    assert_eq!(state.blockchain.read().await.state.state_root_material(), root);
    assert!(state.storage.read().await.archived_node_receipt(&[0;32]).unwrap().is_none());
}

}
