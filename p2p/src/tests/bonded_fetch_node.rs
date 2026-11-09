//! A real node serving the bonded snapshot fetch protocol to the real client
//! (the same source the CLI compiles), over loopback TCP.

use std::time::Duration;

use tokio::time::timeout;

use super::helpers::allocate_port;
use crate::bonded_fetch::{BondedFetchRequest, BondedFetchResponse, client::BondedFetchClient};
use crate::{P2pCommand, P2pConfig, P2pNode};

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

async fn start_node(serve: bool) -> (crate::P2pHandle, libp2p::Multiaddr) {
    let port = allocate_port().await;
    let listen: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port}").parse().unwrap();
    let cfg = P2pConfig {
        listen_addresses: vec![listen.clone()],
        bootstrap_peers: Vec::new(),
        serve_bonded_snapshots: serve,
        ..super::helpers::peering_config()
    };
    let handle = P2pNode::start(cfg).await.expect("node starts");
    let addr = listen.with(libp2p::multiaddr::Protocol::P2p(handle.local_peer_id));
    (handle, addr)
}

#[tokio::test]
async fn a_serving_node_relays_requests_to_the_application_and_its_answers_back() {
    let _guard = super::helpers::serialize_multi_node_test();
    let (mut handle, addr) = start_node(true).await;
    let mut requests = handle.take_bonded_fetch_requests().expect("serving node exposes its queue");
    assert!(handle.take_bonded_fetch_requests().is_none(), "the queue has one owner");
    let commands = handle.command_tx.clone();
    let server_peer = handle.local_peer_id;
    // The "application": answer Info with a fixed identity and record who asked.
    let app = tokio::spawn(async move {
        let inbound = requests.recv().await.expect("one request");
        assert_eq!(inbound.request, BondedFetchRequest::Info);
        commands
            .send(P2pCommand::RespondToBondedFetch {
                request_id: inbound.request_id,
                response: BondedFetchResponse::Info {
                    chain_id: "888".into(),
                    network_id: "testnet".into(),
                    genesis_hash: "0x".to_string() + &"aa".repeat(32),
                    service_id: "laptop".into(),
                },
            })
            .await
            .unwrap();
        inbound.peer_id
    });
    let mut client = timeout(TEST_TIMEOUT, BondedFetchClient::connect(addr, TEST_TIMEOUT))
        .await
        .unwrap()
        .expect("client connects");
    assert_eq!(client.server_peer_id(), server_peer);
    let answer = timeout(TEST_TIMEOUT, client.call(BondedFetchRequest::Info)).await.unwrap().unwrap();
    assert!(matches!(answer, BondedFetchResponse::Info { ref service_id, .. } if service_id == "laptop"));
    let asked_by = timeout(TEST_TIMEOUT, app).await.unwrap().unwrap();
    assert_eq!(asked_by, client.local_peer_id(), "the application sees the authenticated client peer");
    let _ = handle.command_tx.send(P2pCommand::Shutdown).await;
}

#[tokio::test]
async fn a_serving_node_whose_queue_was_never_taken_answers_unavailable() {
    let _guard = super::helpers::serialize_multi_node_test();
    let (mut handle, addr) = start_node(true).await;
    drop(handle.take_bonded_fetch_requests());
    let mut client = timeout(TEST_TIMEOUT, BondedFetchClient::connect(addr, TEST_TIMEOUT))
        .await
        .unwrap()
        .unwrap();
    let answer = timeout(TEST_TIMEOUT, client.call(BondedFetchRequest::Info)).await.unwrap().unwrap();
    assert!(matches!(answer, BondedFetchResponse::Error { ref code, .. } if code == "UNAVAILABLE"), "{answer:?}");
    let _ = handle.command_tx.send(P2pCommand::Shutdown).await;
}

#[tokio::test]
async fn a_node_that_does_not_serve_refuses_the_protocol_at_negotiation() {
    let _guard = super::helpers::serialize_multi_node_test();
    let (mut handle, addr) = start_node(false).await;
    assert!(handle.take_bonded_fetch_requests().is_none());
    let mut client = timeout(TEST_TIMEOUT, BondedFetchClient::connect(addr, TEST_TIMEOUT))
        .await
        .unwrap()
        .unwrap();
    let error = timeout(TEST_TIMEOUT, client.call(BondedFetchRequest::Info)).await.unwrap().unwrap_err();
    assert!(error.contains("does not serve bonded snapshots"), "{error}");
    let _ = handle.command_tx.send(P2pCommand::Shutdown).await;
}

#[test]
fn serving_is_off_unless_the_operator_opts_in() {
    assert_eq!(crate::config::SERVE_BONDED_SNAPSHOTS_ENV, "WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS");
    if std::env::var_os(crate::config::SERVE_BONDED_SNAPSHOTS_ENV).is_none() {
        assert!(!P2pConfig::default().serve_bonded_snapshots);
    }
}
