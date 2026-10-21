//! `wrfcoin snapshot fetch` end to end over mocked servers (core4#3126):
//! a trusted signed snapshot is fetched, verified client-side, ingested into
//! the local node, and the local node's receipt is delivered back to the
//! server; an untrusted signer and a non-reproducing export are refused
//! **before** any request reaches the local node.

use std::process::Command;

use mockito::{Matcher, Server};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde_json::{Value, json};
use wrfcoin_shared_protocol::blockchain::snapshot_sync::{
    build_snapshot_sync_payload, build_state_snapshot, sign_snapshot_sync_payload,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

fn run(args: &[&str], local_url: &str) -> (bool, String, String) {
    // PID alone is shared by all three parallel tests. Config creation truncates
    // the file before writing, so another CLI can observe an empty config.
    struct TestHome(std::path::PathBuf);
    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let home = TestHome(std::env::temp_dir().join(format!(
        "wrfcoin-snapshot-fetch-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    )));
    std::fs::create_dir(&home.0).expect("isolated test HOME");
    let out = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
        .args(args)
        .env("HOME", &home.0)
        .env("RUST_LOG", "error")
        .env("WRFCOIN_API_URL", local_url)
        .env_remove("WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS")
        .output()
        .expect("run wrfcoin");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn fixture() -> (Value, Vec<Value>, String) {
    fixture_for(wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active())
}

fn fixture_for(identity: &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity) -> (Value, Vec<Value>, String) {
    let entries_raw = vec![
        (b"account:alice".to_vec(), b"100".to_vec()),
        (b"account:bob".to_vec(), b"7".to_vec()),
        (b"station:kdlh".to_vec(), b"active".to_vec()),
    ];
    let snapshot = build_state_snapshot(777, "ab".repeat(32), &entries_raw);
    let payload = build_snapshot_sync_payload(&snapshot, "cd".repeat(32), snapshot.height);
    let sk = PrivateKey::generate(&mut StdRng::seed_from_u64(3126));
    let signer = PublicKey::from_private_key(&sk).to_hex().to_lowercase();
    let mut signed = sign_snapshot_sync_payload(payload.clone(), &sk, 1_000, "nonce".to_string());
    signed.signature = sk.sign(signed.canonical_signing_message_for(identity).as_bytes()).to_hex();
    let payload_json = json!({ "payload": payload, "signed_payload": signed });
    let entries: Vec<Value> = entries_raw
        .iter()
        .map(|(k, v)| json!({ "key_hex": hex::encode(k), "value_hex": hex::encode(v) }))
        .collect();
    (payload_json, entries, signer)
}

fn export_page(payload: &Value, entries: &[Value]) -> Value {
    let p = &payload["payload"];
    json!({
        "height": p["height"],
        "state_root_hex": p["state_root_hex"],
        "snapshot_root_hex": p["snapshot_root_hex"],
        "offset": 0,
        "limit": 500,
        "total": entries.len(),
        "next_offset": Value::Null,
        "entries": entries,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trusted_snapshot_is_fetched_verified_ingested_and_receipted() {
    let (payload, entries, signer) = fixture();
    let mut server = Server::new_async().await;
    let mut local = Server::new_async().await;
    let identity = wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active();
    let info = json!({"chain_id":identity.chain_id,"network_id":identity.network_id,"genesis_hash":"ab".repeat(32)});
    let _source_info = network(&mut server, &info).await;
    let _receiver_info = network(&mut local, &info).await;

    let m_payload = server
        .mock("GET", "/state/snapshot/sync-payload")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(payload.to_string())
        .create_async()
        .await;
    let m_export = server
        .mock("GET", Matcher::Regex(r"^/state/snapshot/export.*$".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(export_page(&payload, &entries).to_string())
        .create_async()
        .await;
    let m_receipt = server
        .mock("POST", "/network/snapshot-sync/receipt")
        .match_body(Matcher::PartialJson(
            json!({ "receipt": { "signature": "c2ln" } }),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "accepted": true, "recorded_receipts": 1 }).to_string())
        .create_async()
        .await;
    let m_ingest = local
        .mock("POST", "/network/snapshot-sync/ingest")
        .match_body(Matcher::PartialJson(
            json!({ "served_by": server.host_with_port() }),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "accepted": true, "applied": true, "height": 777,
                "receipt": { "receipt": { "service": "snapshot_join" }, "signature": "c2ln" },
                "receipt_unavailable_reason": Value::Null,
            })
            .to_string(),
        )
        .create_async()
        .await;

    let (ok, stdout, stderr) = run(
        &[
            "--json",
            "snapshot",
            "fetch",
            "--from",
            &server.url(),
            "--trusted-signers",
            &signer,
        ],
        &local.url(),
    );
    assert!(ok, "fetch must succeed: stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("\"delivered\":true") || stdout.contains("\"delivered\": true"),
        "{stdout}"
    );
    m_payload.assert_async().await;
    m_export.assert_async().await;
    m_ingest.assert_async().await;
    m_receipt.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_untrusted_signer_is_refused_before_the_local_node_is_touched() {
    let (payload, entries, _signer) = fixture();
    let mut server = Server::new_async().await;
    let mut local = Server::new_async().await;
    let identity = wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active();
    let info = json!({"chain_id":identity.chain_id,"network_id":identity.network_id,"genesis_hash":"ab".repeat(32)});
    let _source_info = network(&mut server, &info).await;
    let _receiver_info = network(&mut local, &info).await;
    let _p = server
        .mock("GET", "/state/snapshot/sync-payload")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(payload.to_string())
        .create_async()
        .await;
    let _e = server
        .mock("GET", Matcher::Regex(r"^/state/snapshot/export.*$".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(export_page(&payload, &entries).to_string())
        .create_async()
        .await;
    let ingest = local
        .mock("POST", "/network/snapshot-sync/ingest")
        .expect(0)
        .create_async()
        .await;

    let other_signer = "ab".repeat(32);
    let (ok, stdout, stderr) = run(
        &[
            "--json",
            "snapshot",
            "fetch",
            "--from",
            &server.url(),
            "--trusted-signers",
            &other_signer,
        ],
        &local.url(),
    );
    assert!(!ok, "an untrusted signer must be refused: stdout={stdout}");
    assert!(
        (stdout + &stderr).contains("not in the trusted signer list"),
        "refusal must name the reason"
    );
    ingest.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_export_that_does_not_reproduce_the_signed_root_is_refused() {
    let (payload, mut entries, signer) = fixture();
    entries[1]["value_hex"] = json!(hex::encode(b"8"));
    let mut server = Server::new_async().await;
    let mut local = Server::new_async().await;
    let identity = wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active();
    let info = json!({"chain_id":identity.chain_id,"network_id":identity.network_id,"genesis_hash":"ab".repeat(32)});
    let _source_info = network(&mut server, &info).await;
    let _receiver_info = network(&mut local, &info).await;
    let _p = server
        .mock("GET", "/state/snapshot/sync-payload")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(payload.to_string())
        .create_async()
        .await;
    let _e = server
        .mock("GET", Matcher::Regex(r"^/state/snapshot/export.*$".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(export_page(&payload, &entries).to_string())
        .create_async()
        .await;
    let ingest = local
        .mock("POST", "/network/snapshot-sync/ingest")
        .expect(0)
        .create_async()
        .await;

    let (ok, stdout, stderr) = run(
        &[
            "--json",
            "snapshot",
            "fetch",
            "--from",
            &server.url(),
            "--trusted-signers",
            &signer,
        ],
        &local.url(),
    );
    assert!(
        !ok,
        "a non-reproducing export must be refused: stdout={stdout}"
    );
    // Show a sanitized category, not raw child output (which could contain
    // inherited configuration), when the intended verification was not reached.
    let output = stdout + &stderr;
    assert!(
        output.contains("reproduce root"),
        "refusal must name the root mismatch (config parse failure: {}; signature failure: {})",
        output.contains("failed to parse config file"),
        output.contains("snapshot payload signature does not verify")
    );
    ingest.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bonded_dry_run_and_missing_archive_refuse_before_any_fetch() {
    let mut server = Server::new_async().await;
    let no_fetch = server.mock("GET", "/state/snapshot/sync-payload").expect(0).create_async().await;
    for extra in [&["--bonded", "--dry-run"][..], &["--bonded"][..]] {
        let url = server.url();
        let mut args = vec!["snapshot", "fetch", "--from", &url];
        args.extend_from_slice(extra);
        let (ok, stdout, stderr) = run(&args, &url);
        assert!(!ok);
        assert!(format!("{stdout}{stderr}").contains("bonded fetch requires actual installation"));
    }
    no_fetch.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stdin_credentials_are_bound_to_server_reader_and_receiver_operator() {
    use std::{io::Write,process::Stdio};
    let (payload, entries, signer)=fixture();
    let mut source=Server::new_async().await;let mut receiver=Server::new_async().await;
    let info = json!({"chain_id":wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active().chain_id,
        "network_id":wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active().network_id,"genesis_hash":"ab".repeat(32)});
    let _source_info=network(&mut source,&info).await;let _receiver_info=network(&mut receiver,&info).await;
    let source_role="synthetic-server-reader";let receiver_role="synthetic-receiver-operator";
    let payload_mock=source.mock("GET","/state/snapshot/sync-payload").match_header("authorization",format!("Bearer {source_role}").as_str()).with_status(200).with_header("content-type","application/json").with_body(payload.to_string()).create_async().await;
    let export_mock=source.mock("GET",Matcher::Regex(r"^/state/snapshot/export.*$".into())).match_header("authorization",format!("Bearer {source_role}").as_str()).with_status(200).with_header("content-type","application/json").with_body(export_page(&payload,&entries).to_string()).create_async().await;
    let install_mock=receiver.mock("POST","/network/snapshot-sync/ingest").match_header("authorization",format!("Bearer {receiver_role}").as_str()).with_status(200).with_header("content-type","application/json").with_body(json!({"applied":true,"receipt":null,"receipt_unavailable_reason":"synthetic unsigned receiver"}).to_string()).create_async().await;
    let home=std::env::temp_dir().join(format!("node-snapshot-auth-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&home).unwrap();
    let identity=wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active();
    let mut child=Command::new(env!("CARGO_BIN_EXE_wrfcoin")).env_clear().env("HOME",&home).env("WRFCOIN_CHAIN_ID",&identity.chain_id).env("WRFCOIN_NETWORK_ID",&identity.network_id)
        .args(["--json","snapshot","fetch","--from",&source.url(),"--to",&receiver.url(),"--trusted-signers",&signer,"--auth-stdin"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(json!({"server_reader":source_role,"receiver_operator":receiver_role}).to_string().as_bytes()).unwrap();
    let output=tokio::task::spawn_blocking(move||child.wait_with_output().unwrap()).await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    for credential in [source_role,receiver_role] {assert!(!String::from_utf8_lossy(&output.stdout).contains(credential));assert!(!String::from_utf8_lossy(&output.stderr).contains(credential));}
    payload_mock.assert_async().await;export_mock.assert_async().await;install_mock.assert_async().await;
}

async fn network(server: &mut Server, info: &Value) -> mockito::Mock {
    server.mock("GET", "/network/info").with_status(200)
        .with_header("content-type", "application/json").with_body(info.to_string())
        .create_async().await
}

/// Exercise the public binary without a chain flag or 888 environment: the
/// source node supplies the signature domain, and the receiver must match it.
#[tokio::test(flavor = "multi_thread")]
async fn chain_888_fresh_receiver_ingests_but_mismatched_or_invalid_identity_never_does() {
    use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
    let (payload, entries, signer) = fixture_for(&ChainIdentity::new("888", "testnet"));
    let info = json!({"chain_id":"888","network_id":"testnet","genesis_hash":"ab".repeat(32)});
    for case in ["known", "unknown", "chain_id", "network_id", "genesis_hash", "unknown_wrong_chain",
        "unknown_wrong_network", "unknown_chain", "unknown_network", "source_unknown", "missing", "malformed", "zero"] {
        let mut source = Server::new_async().await;
        let mut receiver = Server::new_async().await;
        let mut source_info = info.clone();
        let mut receiver_info = info.clone();
        match case {
            "chain_id" => receiver_info["chain_id"] = json!("777"),
            "network_id" => receiver_info["network_id"] = json!("mainnet"),
            "genesis_hash" => receiver_info["genesis_hash"] = json!("cd".repeat(32)),
            "unknown" => receiver_info["genesis_hash"] = json!("unknown"),
            "unknown_wrong_chain" | "unknown_wrong_network" => {
                receiver_info["genesis_hash"] = json!("unknown");
                if case == "unknown_wrong_chain" { receiver_info["chain_id"] = json!("777"); }
                else { receiver_info["network_id"] = json!("mainnet"); }
            },
            "unknown_chain" => receiver_info["chain_id"] = json!("unknown"),
            "unknown_network" => receiver_info["network_id"] = json!("unknown"),
            "source_unknown" => source_info["genesis_hash"] = json!("unknown"),
            "missing" => { receiver_info.as_object_mut().unwrap().remove("genesis_hash"); },
            "malformed" => receiver_info["genesis_hash"] = json!("invalid"),
            "zero" => receiver_info["genesis_hash"] = json!("00".repeat(32)),
            _ => {},
        }
        let _source_info = network(&mut source, &source_info).await;
        let _receiver_info = network(&mut receiver, &receiver_info).await;
        let _payload = source.mock("GET", "/state/snapshot/sync-payload")
            .expect(if case == "source_unknown" { 0 } else { 1 })
            .with_header("content-type", "application/json").with_body(payload.to_string()).create_async().await;
        let _export = source.mock("GET", Matcher::Regex(r"^/state/snapshot/export.*$".into()))
            .expect(if case == "source_unknown" { 0 } else { 1 })
            .with_header("content-type", "application/json").with_body(export_page(&payload, &entries).to_string()).create_async().await;
        let accepts = matches!(case, "known" | "unknown");
        let ingest = receiver.mock("POST", "/network/snapshot-sync/ingest")
            .expect(if accepts { 1 } else { 0 })
            .with_header("content-type", "application/json").with_body(json!({"applied":true,"receipt":null}).to_string()).create_async().await;
        let (ok, stdout, stderr) = run(&["--json", "snapshot", "fetch", "--from", &source.url(), "--trusted-signers", &signer], &receiver.url());
        assert_eq!(ok, accepts, "case {case}: {stdout}{stderr}");
        if !accepts {
            let reason = match case {
                "unknown_chain" | "unknown_network" | "missing" => "network identity unavailable",
                "source_unknown" | "malformed" | "zero" => "genesis unavailable",
                _ => "chain/genesis mismatch",
            };
            assert!(format!("{stdout}{stderr}").contains(reason), "case {case}");
        }
        ingest.assert_async().await;
        if case == "source_unknown" { _payload.assert_async().await; _export.assert_async().await; }
    }
}

/// The exact bonded command must reach the receiver-ingest POST on a fresh
/// receiver. A receipt-less mock then refuses settlement, never fabricating proof.
#[tokio::test(flavor = "multi_thread")]
async fn bonded_888_fresh_receiver_reaches_ingest_once() {
    use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
    let (payload, entries, signer) = fixture_for(&ChainIdentity::new("888", "testnet"));
    let mut source = Server::new_async().await;
    let mut receiver = Server::new_async().await;
    let _source_info = network(&mut source, &json!({"chain_id":"888","network_id":"testnet","genesis_hash":"ab".repeat(32)})).await;
    let _receiver_info = network(&mut receiver, &json!({"chain_id":"888","network_id":"testnet","genesis_hash":"unknown"})).await;
    let _payload = source.mock("GET", "/state/snapshot/sync-payload")
        .with_header("content-type", "application/json").with_body(payload.to_string()).create_async().await;
    let _export = source.mock("GET", Matcher::Regex(r"^/state/snapshot/export.*$".into()))
        .with_header("content-type", "application/json").with_body(export_page(&payload, &entries).to_string()).create_async().await;
    let ingest = receiver.mock("POST", "/network/snapshot-sync/receiver-ingest").expect(1)
        .with_header("content-type", "application/json").with_body(json!({"applied":true,"receipt":null}).to_string()).create_async().await;
    let no_ticket = source.mock("POST", "/network/snapshot-sync/bonded-ticket").expect(0).create_async().await;
    let archive = std::env::temp_dir().join(format!("fresh-receiver-public-receipts-{}", uuid::Uuid::new_v4()));
    let (ok, stdout, stderr) = run(&["--json", "snapshot", "fetch", "--from", &source.url(), "--to", &receiver.url(),
        "--trusted-signers", &signer, "--bonded", "--receipt-archive", archive.to_str().unwrap()], &receiver.url());
    assert!(!ok);
    assert!(format!("{stdout}{stderr}").contains("installation returned no signed receipt"));
    ingest.assert_async().await;
    no_ticket.assert_async().await;
    assert!(!archive.exists(), "no receipt may be archived without real evidence");
}

#[test]
fn node_pay_signing_commands_require_explicit_chain_even_for_dry_run() {
    let deposit = ["node", "bond", "deposit", "--deployment-id", "01", "--owner-key", "02",
        "--payout-address", "03", "--payout-key", "04", "--server-key", "05", "--consumer-key", "06",
        "--governance-key", "07", "--service-alias", "synthetic", "--genesis-hash", "08",
        "--activation-height", "9", "--api-url", "http://127.0.0.1:1", "--operator-token-file", "absent", "--dry-run"];
    let receipt = ["node", "receipt", "submit", "--archive", "absent", "--epoch", "1",
        "--deployment-id", "01", "--sender", "02", "--genesis-hash", "03",
        "--api-url", "http://127.0.0.1:1", "--dry-run"];
    for args in [&deposit[..], &receipt[..]] {
        let (ok, stdout, stderr) = run(args, "http://127.0.0.1:1");
        assert!(!ok);
        assert!(format!("{stdout}{stderr}").contains("--chain-id <CHAIN_ID>"));
        for chain in ["777", "888"] {
            let mut explicit = args.to_vec();explicit.extend(["--chain-id", chain]);
            let (_, stdout, stderr) = run(&explicit, "http://127.0.0.1:1");
            assert!(!format!("{stdout}{stderr}").contains("required arguments were not provided"));
        }
    }
}

#[path = "../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod receipt_fixture;

#[tokio::test(flavor = "multi_thread")]
async fn retained_receipt_packet_is_verified_against_target_before_any_post() {
    let state = receipt_fixture::state();
    let packet = receipt_fixture::transaction(&receipt_fixture::submission(
        vec![receipt_fixture::claim(&state, 1)], 1));
    let path = std::env::temp_dir().join(format!("public-receipt-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&path, serde_json::to_vec(&packet).unwrap()).unwrap();
    let original = json!({"chain_id":"777","network_id":"testnet","genesis_hash":hex::encode([7;32])});
    for change in [None, Some("chain_id"), Some("network_id"), Some("genesis_hash"), Some("unknown")] {
        let mut target = Server::new_async().await;
        let mut info = original.clone();
        if let Some(field) = change {
            let key = if field == "unknown" { "genesis_hash" } else { field };
            info[key] = match field {
                "chain_id" => json!("888"), "network_id" => json!("mainnet"), "unknown" => json!("unknown"), _ => json!(hex::encode([8;32])),
            };
        }
        let _info = network(&mut target, &info).await;
        let post = target.mock("POST", "/node/receipts/submit")
            .expect(if change.is_none() { 1 } else { 0 })
            .with_header("content-type", "application/json")
            .with_body(json!({"tx_id":packet.id.unwrap().to_hex(),"admitted":true}).to_string())
            .create_async().await;
        let (ok, stdout, stderr) = run(&["--json", "snapshot", "submit-receipt", "--transaction-file", path.to_str().unwrap()], &target.url());
        assert_eq!(ok, change.is_none(), "case {change:?}: {stdout}{stderr}");
        if change.is_some() { assert!(format!("{stdout}{stderr}").contains(if change == Some("unknown") { "target genesis unavailable" } else { "receipt target chain/genesis mismatch" })); }
        post.assert_async().await;
    }
    std::fs::remove_file(path).unwrap();
}
