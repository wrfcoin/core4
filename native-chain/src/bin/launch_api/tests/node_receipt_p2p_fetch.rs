//! Owner decision 2026-10-31: a bonded fetch over P2P. A real server swarm
//! (`P2pNode`, serving the protocol) answers through the production
//! `p2p_bonded_fetch` handler; the client is the one the CLI compiles. The
//! claim it produces must be byte-identical to the HTTP route's and verify
//! with the same contract check.
use super::*;
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
use super::*;
use wrfcoin_shared_protocol::consensus::node_service_receipt::SignedNodeServiceReceipt;
use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
use wrfcoin_p2p::bonded_fetch::{
    BondedFetchAuth, BondedFetchRequest, BondedFetchResponse, client::BondedFetchClient,
};

fn refused(response: &BondedFetchResponse) -> &str {
    match response {
        BondedFetchResponse::Error { code, .. } => code,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[actix_web::test]
async fn bonded_claim_over_p2p_matches_and_verifies_like_http() {
    const NAME: &str = "tests::node_receipt_p2p_fetch::tests::bonded_claim_over_p2p_matches_and_verifies_like_http";
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
        crypto::chain_binding::{
            ActivationTable, FeatureSchedule, SigningProtocolVersion, install_activation_table,
            install_feature_schedule, try_activation_table,
        },
    };
    install_feature_schedule(FeatureSchedule::from_entries(vec![]).unwrap());
    // The receipt names the server by its service id; the bond's alias is "node-1".
    // Set only inside this test's own isolated child process.
    std::env::set_var("WRFCOIN_SERVICE_ID", "node-1");
    let headers = crate::routes::snapshot::snapshot_test_support::chain_committing_to("placeholder", fixture::H);
    let genesis = headers[0].hash.unwrap();
    let genesis_bytes = *genesis.as_bytes();
    let mut bonded = fixture::bonds::state_on(
        genesis_bytes,
        *headers[(fixture::bonds::A - 2) as usize].hash.unwrap().as_bytes(),
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
    let server = test_state(Some(fixture::key(11)));
    let mut receiver_inner = test_state(Some(fixture::key(22))).into_inner();
    Arc::get_mut(&mut receiver_inner).unwrap().snapshot_trusted_signers =
        Arc::new([hex::encode(fixture::pk(11))].into_iter().collect());
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

    // A real swarm refuses to start without a signing version and schedule
    // (#2275, #1396). Install exactly what this process already falls back to,
    // so no height changes version.
    let version = SigningProtocolVersion::install_active(SigningProtocolVersion::active());
    if try_activation_table().is_none() {
        install_activation_table(ActivationTable::from_entries(vec![(0, version)]).unwrap());
    }
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let listen: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port}").parse().unwrap();
    let handshake_genesis = hex::encode(
        wrfcoin_shared_protocol::crypto::hash::hash_string("wrfcoin-p2p-bonded-fetch-test-genesis").as_bytes(),
    );
    let mut handle = wrfcoin_p2p::P2pNode::start(wrfcoin_p2p::P2pConfig {
        listen_addresses: vec![listen.clone()],
        bootstrap_peers: Vec::new(),
        serve_bonded_snapshots: true,
        genesis_hash: handshake_genesis,
        ..wrfcoin_p2p::P2pConfig::default()
    })
    .await
    .expect("server swarm starts");
    let requests = handle.take_bonded_fetch_requests().expect("serving swarm exposes its queue");
    let server_peer = handle.local_peer_id;
    let (p2p_client, events) = handle.into_parts();
    drop(events);
    crate::p2p_bonded_fetch::spawn_bonded_fetch_server(requests, server.clone(), p2p_client.clone());

    let addr = listen.with(libp2p::multiaddr::Protocol::P2p(server_peer));
    let mut fetch = BondedFetchClient::connect(addr, std::time::Duration::from_secs(30)).await.expect("client connects");
    let me = fetch.local_peer_id().to_string();

    // Info is open; it names the identity and the service id receipts must use.
    let BondedFetchResponse::Info { chain_id, network_id, genesis_hash, service_id } =
        fetch.call(BondedFetchRequest::Info).await.unwrap()
    else {
        panic!("info")
    };
    assert_eq!((chain_id.as_str(), network_id.as_str()), ("777", "testnet"));
    assert_eq!(genesis_hash, format!("0x{}", hex::encode(genesis_bytes)));
    assert_eq!(service_id, "node-1");

    // The receiver signs the fetch authorization; it is Operator-only.
    let auth_body = json!({"server_peer_id": server_peer.to_string(), "client_peer_id": me});
    let anonymous = test::call_service(&receiver_app, test::TestRequest::post()
        .uri("/network/snapshot-sync/p2p-fetch-auth").set_json(&auth_body).to_request()).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let reader = test::call_service(&receiver_app, test::TestRequest::post()
        .uri("/network/snapshot-sync/p2p-fetch-auth")
        .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test principal
        .set_json(&auth_body).to_request()).await;
    assert_eq!(reader.status(), StatusCode::FORBIDDEN);
    let auth: BondedFetchAuth = test::read_body_json(test::call_service(&receiver_app, test::TestRequest::post()
        .uri("/network/snapshot-sync/p2p-fetch-auth")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test principal
        .set_json(&auth_body).to_request()).await).await;
    assert_eq!(auth.consumer_key, hex::encode(fixture::pk(22)));

    // Refusals: no request reaches snapshot work without a bonded consumer's
    // authorization for exactly this client peer.
    let mut unbonded = auth.clone();
    {
        let key = fixture::key(33);
        let public: [u8; 32] = fixture::pk(33);
        let nonce = [9u8; 16];
        unbonded.consumer_key = hex::encode(public);
        unbonded.nonce = hex::encode(nonce);
        unbonded.signature = key
            .sign(&BondedFetchAuth::signing_preimage(
                ChainIdentity::active(),
                &public,
                &unbonded.server_peer_id,
                &unbonded.client_peer_id,
                unbonded.issued_at,
                &nonce,
            ))
            .to_hex();
    }
    let answer = fetch.call(BondedFetchRequest::SyncPayload { auth: unbonded }).await.unwrap();
    assert_eq!(refused(&answer), "NOT_BONDED_CONSUMER");
    let mut forged = auth.clone();
    forged.issued_at += 1;
    assert_eq!(refused(&fetch.call(BondedFetchRequest::SyncPayload { auth: forged }).await.unwrap()), "AUTH_SIGNATURE_INVALID");
    let stolen: BondedFetchAuth = test::read_body_json(test::call_service(&receiver_app, test::TestRequest::post()
        .uri("/network/snapshot-sync/p2p-fetch-auth")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test principal
        .set_json(json!({"server_peer_id": server_peer.to_string(), "client_peer_id": libp2p::PeerId::random().to_string()}))
        .to_request()).await).await;
    assert_eq!(refused(&fetch.call(BondedFetchRequest::SyncPayload { auth: stolen }).await.unwrap()), "AUTH_WRONG_CLIENT");

    // 1. Payload and pages (small pages, so the cut is read in several requests).
    let BondedFetchResponse::SyncPayload { payload, signed_payload: signed } =
        fetch.call(BondedFetchRequest::SyncPayload { auth: auth.clone() }).await.unwrap()
    else {
        panic!("sync payload")
    };
    assert_eq!(signed.payload, payload);
    assert_eq!(signed.signer_id.to_lowercase(), hex::encode(fixture::pk(11)));
    let mut entries = Vec::new();
    let mut offset = 0u64;
    let mut pages = 0;
    loop {
        let page = fetch.call(BondedFetchRequest::ExportPage {
            auth: auth.clone(),
            height: payload.height,
            snapshot_root_hex: payload.snapshot_root_hex.clone(),
            offset,
            limit: 3,
        }).await.unwrap();
        let BondedFetchResponse::ExportPage { height, snapshot_root_hex, total, next_offset, entries: page } = page else {
            panic!("export page: {page:?}")
        };
        assert_eq!((height, snapshot_root_hex.as_str()), (payload.height, payload.snapshot_root_hex.as_str()));
        assert!(page.len() <= 3);
        entries.extend(page.into_iter().map(|e| json!({"key_hex": e.key_hex, "value_hex": e.value_hex})));
        pages += 1;
        match next_offset {
            Some(next) => offset = next,
            None => {
                assert_eq!(entries.len() as u64, total);
                break;
            },
        }
    }
    assert!(pages > 1, "the fixture must take several pages");
    assert_eq!(entries.len() as u64, signed.payload.entry_count);
    let unknown_cut = fetch.call(BondedFetchRequest::ExportPage {
        auth: auth.clone(), height: payload.height, snapshot_root_hex: "00".repeat(32), offset: 0, limit: 3,
    }).await.unwrap();
    assert_eq!(refused(&unknown_cut), "SNAPSHOT_CUT_EXPIRED");

    // 2. Install on the receiver; it signs the receipt naming the P2P service id.
    let installed = test::call_service(&receiver_app, test::TestRequest::post()
        .uri("/network/snapshot-sync/receiver-ingest")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test principal
        .set_json(json!({"signed_payload": signed, "entries": entries, "served_by": service_id}))
        .to_request()).await;
    let installed_status = installed.status();
    let installed: serde_json::Value = test::read_body_json(installed).await;
    assert_eq!(installed_status, StatusCode::OK, "{installed}");
    let receipt: SignedNodeServiceReceipt = serde_json::from_value(installed["receipt"].clone()).unwrap();
    assert_eq!(receipt.receipt.consumer_pubkey, fixture::pk(22));

    // 3. The server's ticket over P2P. A receipt naming another consumer is
    // refused before any signing.
    let mut other = receipt.clone();
    other.receipt.consumer_pubkey = fixture::pk(33);
    let wrong = fetch.call(BondedFetchRequest::BondedTicket { auth: auth.clone(), receipt: other, manifest: signed.clone() }).await.unwrap();
    assert_eq!(refused(&wrong), "NODE_TICKET_WRONG_CONSUMER");
    let BondedFetchResponse::BondedTicket { claim_hex: p2p_claim } = fetch
        .call(BondedFetchRequest::BondedTicket { auth: auth.clone(), receipt: receipt.clone(), manifest: signed.clone() })
        .await
        .unwrap()
    else {
        panic!("ticket")
    };
    // The same installed receipt through the HTTP route yields the same bytes.
    let archive = wrfcoin_native_chain::node_service_archive::InstalledNodeReceipt { receipt: receipt.clone(), manifest: signed.clone() };
    let http_ticket = test::call_service(&server_app, test::TestRequest::post()
        .uri("/network/snapshot-sync/bonded-ticket")
        .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed synthetic test principal
        .set_json(&archive).to_request()).await;
    assert_eq!(http_ticket.status(), StatusCode::OK);
    let http_ticket: serde_json::Value = test::read_body_json(http_ticket).await;
    assert_eq!(http_ticket["claim_hex"].as_str(), Some(p2p_claim.as_str()), "P2P and HTTP tickets are the same claim");

    // 4. The receiver endorses, exactly as after an HTTP fetch.
    let endorsed = test::call_service(&receiver_app, test::TestRequest::post()
        .uri("/network/snapshot-sync/bonded-endorsement")
        .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test principal
        .set_json(json!({"claim_hex": p2p_claim})).to_request()).await;
    assert_eq!(endorsed.status(), StatusCode::OK);
    let endorsed: serde_json::Value = test::read_body_json(endorsed).await;
    let claim = BondedServiceClaim::from_wire(&hex::decode(endorsed["claim_hex"].as_str().unwrap()).unwrap()).unwrap();
    claim
        .verify_contract(
            &fixture::identity(),
            &genesis_bytes,
            &fixture::pk(11),
            &fixture::pk(22),
            wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(fixture::H),
        )
        .unwrap();
    let chain = server.blockchain.read().await;
    verify_claim(&chain.state, &claim, chain.height + 1, &fixture::identity()).expect("the consensus verifier accepts the P2P claim");
    drop(chain);
    assert_eq!(endorsed["receipt_id"], hex::encode(claim_hash(&claim).unwrap()));

    // 5. The receipt goes back over P2P into the server's evidence ledger, once.
    let ack = fetch.call(BondedFetchRequest::Receipt { auth: auth.clone(), receipt: receipt.clone() }).await.unwrap();
    assert_eq!(ack, BondedFetchResponse::ReceiptAck { accepted: true, recorded_receipts: 1 });
    let again = fetch.call(BondedFetchRequest::Receipt { auth, receipt }).await.unwrap();
    assert_eq!(refused(&again), "RECEIPT_DUPLICATE");

    let _ = p2p_client.command_tx.send(wrfcoin_p2p::P2pCommand::Shutdown).await;
}
}
