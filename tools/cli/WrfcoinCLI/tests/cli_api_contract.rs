//! Live-shape CLI↔API contract test (#2328).
//!
//! ## The gap this closes
//!
//! No test anywhere asserted that the CLI's requests are accepted, and its
//! responses correctly parsed, by the API that will actually be deployed. The
//! sibling `api_client_integration.rs` mocks **hand-written** responses, so it
//! proves request construction but cannot catch drift between what the CLI reads
//! and what a real server sends. #2318 removed a broken live-smoke leg; removing a
//! broken attempt at verification does not produce verification (#2328).
//!
//! ## What this test pins
//!
//! It exercises the real `wrfcoin_cli::ApiClient` at the HTTP boundary against two
//! **recorded** server contracts (via `mockito`, so it needs no running server and
//! runs in-process):
//!
//!  1. `backend_*` fixtures — the shape the CLI was built for: the Node backend
//!     (`wrfcoin/backend`), which serves `/api/network/info` + `/api/stats` as
//!     camelCase inside a `{success, data}` envelope. Recorded from the route
//!     surface documented in `docs/ci/CLI_LIVE_CONTRACT_COVERAGE.md` (#2326). The
//!     CLI reads real values from it — this is the contract that must not drift.
//!
//!  2. `launch_api_network_info.json` — core4 `launch_api`'s **supported** response
//!     shape, recorded from
//!     `native-chain/src/bin/launch_api/routes/network.rs::get_network_info`:
//!     snake_case `chain_id`/`network_id`/`chain_height`, no envelope. The CLI
//!     must read semantic values from this node-native contract rather than
//!     silently substituting defaults.
//!
//! Either contract drifting breaks its own assertion here. The backend fixture
//! preserves compatibility for explicit backend URLs while the launch_api fixture
//! pins the default node contract.
//!
//! ## Run locally
//!
//! ```sh
//! cargo test -p wrfcoin_cli --test cli_api_contract
//! ```
//!
//! No server, secret, or network is required.

use mockito::Server;
use std::time::Duration;
use wrfcoin_cli::{ApiClient, ApiConfig};

const BACKEND_NETWORK_INFO: &str = include_str!("fixtures/backend_network_info.json");

#[tokio::test(flavor = "multi_thread")]
async fn forecast_submission_uses_files_canonical_hash_and_bearer() -> anyhow::Result<()> {
    use wrfcoin_cli::forecast_submission::*;
    use wrfcoin_shared_protocol::{blockchain::transaction::{ForecastArtifactManifest,
        ForecastVerificationReceipt, ForecastAccuracyMetrics, Transaction},
        crypto::{signature::{PrivateKey, PublicKey}, domain_signing::{signing_chain_id, signing_network_id}}};
    use rand::SeedableRng;
    let dir = std::env::temp_dir().join(format!("forecast-cli-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir)?;
    let data = dir.join("data.json");
    std::fs::write(&data, r#"{"temperature":20}"#)?;
    let input = ForecastInput { forecaster: "wrf1forecast".into(), station_id: "station-forecast".into(),
        data_file: data, nonce: "reveal-nonce".into(), timestamp: 1700000000 };
    let signer = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(3065));
    let commit = commit_body(&input, &signer, &signing_chain_id(), &signing_network_id())?;
    let id = Transaction::compute_commit_id(&serde_json::json!({"temperature":20}),
        "reveal-nonce", "station-forecast", 1700000000, "forecast");
    assert_eq!(commit["commit_hash"], id);
    let manifest = ForecastArtifactManifest { schema_version: 1, model_id: "cli-test".into(),
        input_hash: "a".repeat(64), output_hash: "b".repeat(64), produced_at: 1700000000,
        region: "test".into(), horizon_hours: 6, input_variables: vec!["temperature".into()],
        output_variables: vec!["temperature".into()] };
    let mut receipt = ForecastVerificationReceipt { manifest_digest: manifest.manifest_digest(),
        commit_id: id.clone(), oracle_id: PublicKey::from_private_key(&signer).to_hex(),
        metrics: ForecastAccuracyMetrics { mae: 1.0, rmse: 1.0, bias: 0.0, skill_score: 0.8 },
        verified_at: 1700000000, nonce: "receipt-nonce".into(), zk_proof: None, signature: None };
    receipt.signature = Some(signer.sign(receipt.signing_message().as_bytes()).to_hex());
    let manifest_path = dir.join("manifest.json");
    let receipt_path = dir.join("receipt.json");
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
    std::fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;
    let reveal = reveal_body(&input, &manifest_path, Some(&receipt_path))?;
    assert_eq!(reveal["verification_receipt"], serde_json::to_value(&receipt)?);
    assert_eq!(reveal["manifest"], serde_json::to_value(&manifest)?);
    assert_eq!(reveal["commit_hash"], id);
    assert!(reveal_body(&input, &manifest_path, None)?["verification_receipt"].is_null());
    let mut server = Server::new_async().await;
    let connection = ForecastConnection { node_url: Some(server.url()), token: Some("test-bearer".into()) };
    let key_path = dir.join("signer.key");
    std::fs::write(&key_path, signer.to_hex())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644))?;
        assert!(read_signer(&key_path).is_err(), "world-readable keys must be refused");
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        assert_eq!(read_signer(&key_path)?.to_hex(), signer.to_hex());
    }
    for (kind, body) in [("commit", commit), ("reveal", reveal)] {
        let mock = server.mock("POST", format!("/transactions/forecast/{kind}").as_str())
            .match_header("authorization", "Bearer test-bearer")
            .match_body(mockito::Matcher::Json(body.clone()))
            .with_status(201).with_header("content-type", "application/json")
            .with_body(r#"{"id":"accepted-tx","tx_type":"ForecastCommit"}"#).expect(2).create_async().await;
        let result = submit(&connection, kind, body).await?;
        assert_eq!(result["commit_id"], id);
        assert_eq!(result["tx_id"], "accepted-tx");
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_wrfcoin"));
        command.env("HOME", &dir).env("WRFCOIN_API_URL", "invalid-url").args(["--output", "json", "forecast", kind,
            "--node-url", &server.url(), "--token", "test-bearer",
            "--forecaster", "wrf1forecast", "--station-id", "station-forecast",
            "--nonce", "reveal-nonce", "--timestamp", "1700000000", "--data-file"])
            .arg(&input.data_file);
        if kind == "commit" {
            command.arg("--signer-key").arg(&key_path)
                .args(["--chain-id", &signing_chain_id(), "--network-id", &signing_network_id()]);
        } else {
            command.arg("--manifest-file").arg(&manifest_path).arg("--receipt-file").arg(&receipt_path);
        }
        let output = command.output()?;
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let rendered: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        assert!(rendered.to_string().contains("accepted-tx"));
        mock.assert_async().await;
    }
    assert!(reveal_body(&input, &manifest_path, Some(&dir.join("missing"))).is_err());
    std::fs::write(&receipt_path, "invalid JSON")?;
    assert!(reveal_body(&input, &manifest_path, Some(&receipt_path)).is_err());
    std::fs::write(&key_path, "invalid-private-key-canary")?;
    let error = read_signer(&key_path).err().expect("invalid key must fail").to_string();
    assert!(!error.contains("invalid-private-key-canary"));
    receipt.commit_id = "0".repeat(64);
    std::fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;
    assert!(reveal_body(&input, &manifest_path, Some(&receipt_path)).is_err());
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
const BACKEND_STATS: &str = include_str!("fixtures/backend_stats.json");
const LAUNCH_API_NETWORK_INFO: &str = include_str!("fixtures/launch_api_network_info.json");
const LAUNCH_API_ADDRESS: &str = include_str!("fixtures/launch_api_address.json");

fn client(base_url: String) -> ApiClient {
    ApiClient::new(ApiConfig {
        base_url,
        timeout: Duration::from_secs(2),
        retry_attempts: 0,
        retry_delay: Duration::from_millis(1),
        explain_mode: false,
        bearer_token: None,
    })
    .expect("ApiClient::new")
}

#[test]
fn launch_api_address_fixture_pins_decimal_nonce() {
    let response: serde_json::Value =
        serde_json::from_str(LAUNCH_API_ADDRESS).expect("launch API address fixture must be JSON");
    let data = response["data"]
        .as_object()
        .expect("address fixture must use the success envelope");
    assert_eq!(data["nonce"], "7");
    assert_eq!(data.len(), 8, "address fixture field set drifted");
}

/// The SUPPORTED contract: against the backend's recorded shape (camelCase,
/// `{success, data}` envelope, `/api` prefix), the CLI reads real values. If the
/// backend renames a field the CLI reads, or the CLI changes which field it reads,
/// this fails — which is the point.
#[tokio::test(flavor = "multi_thread")]
async fn cli_reads_the_supported_backend_contract() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let _info = server
        .mock("GET", "/api/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(BACKEND_NETWORK_INFO)
        .create_async()
        .await;
    let _stats = server
        .mock("GET", "/api/stats")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(BACKEND_STATS)
        .create_async()
        .await;

    // The backend mounts its routes under `/api`.
    let status = client(format!("{}/api", server.url()))
        .get_blockchain_status()
        .await?;

    assert_eq!(status.block_height, 1024, "reads data.chainId");
    assert_eq!(status.network, "wrfcoin-testnet", "reads data.networkName");
    assert_eq!(status.version, "PoU", "reads data.consensusType");
    assert_eq!(status.transactions_count, 12345, "reads data.totalTransactions");
    assert_eq!(status.nodes_count, 12, "reads data.totalNodes");
    Ok(())
}

/// The supported node contract (#2328): the CLI reads launch_api's raw
/// snake_case response without an `/api` prefix or response envelope.
#[tokio::test(flavor = "multi_thread")]
async fn cli_reads_the_supported_launch_api_contract() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let _info = server
        .mock("GET", "/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(LAUNCH_API_NETWORK_INFO)
        .create_async()
        .await;
    // launch_api exposes aggregate transaction counts at `/mining/stats`.
    let _stats = server
        .mock("GET", "/mining/stats")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"avg_block_time_secs":42.0,"chain_height":12345,"difficulty":1,"total_transactions":543}"#,
        )
        .create_async()
        .await;

    // launch_api serves its routes at the root — no `/api` prefix.
    let status = client(server.url()).get_blockchain_status().await?;

    assert_eq!(status.block_height, 12345, "reads chain_height");
    assert_eq!(status.network, "wrfcoin-testnet", "reads network_id");
    assert_eq!(status.version, "0.1.0", "reads node_version");
    assert_eq!(status.nodes_count, 0, "reads peer_count");
    assert_eq!(status.transactions_count, 543, "reads total_transactions");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn launch_api_shape_drift_is_an_error_not_silent_defaults() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let _info = server
        .mock("GET", "/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"chain_id":"777","genesis_hash":"0xdeadbeef"}"#)
        .create_async()
        .await;
    let error = client(server.url())
        .get_blockchain_status()
        .await
        .expect_err("missing launch_api semantic fields must fail");
    assert!(
        error.to_string().contains("chain_height"),
        "unexpected error: {error}"
    );
    Ok(())
}

/// The signer's output is exactly what the reveal path accepts (#3149).
///
/// ADR-004 §2.2: the consensus half of the forecast lane is live at height 0, but
/// nothing produced receipts, so every real reveal scored at
/// `BASELINE_FORECAST_SKILL`. `forecast_submission::reveal_body` re-derives the
/// commit id and manifest digest and checks signature encoding, so a receipt built by
/// hand against a stale understanding of any of those is dropped. This pins
/// producer and consumer to each other: the receipt goes in through
/// `forecast_receipt::sign_receipt` and out through `reveal_body`, with nothing
/// hand-assembled in between.
#[cfg(debug_assertions)]
#[tokio::test(flavor = "multi_thread")]
async fn a_signed_receipt_is_accepted_by_the_reveal_path_unchanged_3149() -> anyhow::Result<()> {
    use rand::SeedableRng;
    use wrfcoin_cli::forecast_receipt::{sign_receipt, sign_receipt_for_input, write_receipt};
    use wrfcoin_cli::forecast_submission::{ForecastInput, reveal_body};
    use wrfcoin_shared_protocol::{
        blockchain::transaction::{
            ChainBinding, ForecastAccuracyMetrics, ForecastArtifactManifest, Transaction,
        },
        crypto::{
            signature::{PrivateKey, PublicKey},
            signer::EnvKeySigner,
        },
    };

    let dir = std::env::temp_dir().join(format!("forecast-receipt-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir)?;
    let data = dir.join("data.json");
    std::fs::write(&data, r#"{"temperature":20}"#)?;
    let input = ForecastInput {
        forecaster: "wrf1forecast".into(),
        station_id: "station-forecast".into(),
        data_file: data,
        nonce: "reveal-nonce".into(),
        timestamp: 1_700_000_000,
    };
    let commit_id = Transaction::compute_commit_id(
        &serde_json::json!({"temperature":20}),
        "reveal-nonce",
        "station-forecast",
        1_700_000_000,
        "forecast",
    );

    let manifest = ForecastArtifactManifest {
        schema_version: 1,
        model_id: "diurnal-anomaly/v1".into(),
        input_hash: "a".repeat(64),
        output_hash: "b".repeat(64),
        produced_at: 1_700_000_000,
        region: "test".into(),
        horizon_hours: 6,
        input_variables: vec!["temperature".into()],
        output_variables: vec!["temperature".into()],
    };
    let manifest_path = dir.join("manifest.json");
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;

    // The oracle key. In production this is a genesis oracle-set member, which is
    // what makes the reveal score at the receipt's skill instead of the baseline.
    let oracle = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(3149));
    let binding = ChainBinding::new("777", "testnet")?;
    let receipt = sign_receipt_for_input(
        &input,
        &manifest,
        // Well above BASELINE_FORECAST_SKILL (0.1) — the reason a receipt is worth
        // producing at all.
        ForecastAccuracyMetrics {
            mae: 0.8,
            rmse: 1.1,
            bias: -0.2,
            skill_score: 0.87,
        },
        1_700_000_100,
        Some("receipt-nonce"),
        &EnvKeySigner::from_private_key(oracle.clone(), "test-oracle"),
        &binding,
    )?;
    let receipt_path = dir.join("receipt.json");
    write_receipt(&receipt, &receipt_path)?;

    let reveal = reveal_body(&input, &manifest_path, Some(&receipt_path))?;
    assert_eq!(
        reveal["verification_receipt"],
        serde_json::to_value(&receipt)?
    );
    assert_eq!(reveal["commit_hash"], commit_id);
    assert_eq!(
        reveal["verification_receipt"]["oracle_id"],
        serde_json::Value::from(PublicKey::from_private_key(&oracle).to_hex()),
        "the receipt names the key that signed it, which is the identity the chain \
         checks against the oracle set"
    );
    assert!(
        reveal["verification_receipt"]["metrics"]["skill_score"]
            .as_f64()
            .is_some_and(|s| s > 0.1),
        "a receipt that does not beat BASELINE_FORECAST_SKILL earns nothing extra"
    );

    // The consumer's own binding checks, restated against a receipt for a different
    // commit: the reveal path must reject it even though it is validly signed.
    let other_commit = Transaction::compute_commit_id(
        &serde_json::json!({"temperature":21}),
        "reveal-nonce",
        "station-forecast",
        1_700_000_000,
        "forecast",
    );
    let stray = sign_receipt(
        &manifest,
        &other_commit,
        ForecastAccuracyMetrics {
            mae: 0.8,
            rmse: 1.1,
            bias: -0.2,
            skill_score: 0.87,
        },
        1_700_000_100,
        "receipt-nonce",
        &EnvKeySigner::from_private_key(oracle.clone(), "test-oracle"),
        &binding,
    )?;
    let stray_path = dir.join("stray.json");
    write_receipt(&stray, &stray_path)?;
    assert!(
        reveal_body(&input, &manifest_path, Some(&stray_path)).is_err(),
        "a receipt for another commit must not ride this reveal"
    );

    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// Exercise public argument parsing, identity refusal and filesystem custody.
#[cfg(all(unix, debug_assertions))]
#[test]
fn forecast_receipt_cli_offline_boundaries_3149() -> anyhow::Result<()> {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    use wrfcoin_cli::forecast_submission::{ForecastInput, reveal_body};
    use wrfcoin_shared_protocol::blockchain::transaction::{
        ChainBinding, ForecastVerificationReceipt,
    };
    let dir = std::env::temp_dir().join(format!("receipt-cli-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir)?;
    let data = dir.join("data.json");
    let manifest = dir.join("manifest.json");
    let metrics = dir.join("metrics.json");
    let key = dir.join("rehearsal.key");
    let out = dir.join("receipt.json");
    std::fs::write(&data, r#"{"temperature":20}"#)?;
    std::fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "schema_version":1,"model_id":"synthetic-model","input_hash":"a".repeat(64),
            "output_hash":"b".repeat(64),"produced_at":1700000000,"region":"synthetic-region",
            "horizon_hours":6,"input_variables":["temperature"],"output_variables":["temperature"]
        }))?,
    )?;
    std::fs::write(
        &metrics,
        r#"{"mae":0.8,"rmse":1.1,"bias":-0.2,"skill_score":0.87}"#,
    )?;
    let synthetic_key = "11".repeat(32);
    std::fs::write(&key, &synthetic_key)?;
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600))?;
    let base = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_wrfcoin"));
        c.env_clear()
            .env("HOME", &dir)
            .env("WRFCOIN_CHAIN_ID", "777")
            .env("WRFCOIN_NETWORK_ID", "testnet")
            .args([
                "forecast",
                "sign-receipt",
                "--forecaster",
                "synthetic-forecaster",
                "--station-id",
                "synthetic-station",
                "--nonce",
                "commit-nonce",
                "--timestamp",
                "1700000000",
                "--chain-id",
                "777",
                "--network-id",
                "testnet",
            ])
            .arg("--data-file")
            .arg(&data)
            .arg("--manifest-file")
            .arg(&manifest)
            .arg("--metrics-file")
            .arg(&metrics)
            .arg("--out")
            .arg(&out);
        c
    };
    let rehearsal = || {
        let mut c = base();
        c.arg("--rehearsal").arg("--oracle-key").arg(&key);
        c
    };
    for variable in ["WRFCOIN_CHAIN_ID", "WRFCOIN_NETWORK_ID"] {
        let result = rehearsal().env_remove(variable).output()?;
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains(variable));
        assert!(!out.exists());
    }
    assert!(
        !rehearsal()
            .env("WRFCOIN_CHAIN_ID", "777 ")
            .output()?
            .status
            .success()
    );
    assert!(
        !base().output()?.status.success(),
        "production must not fall back to a software key"
    );
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644))?;
    assert!(
        !rehearsal().output()?.status.success(),
        "world-readable key must refuse"
    );
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600))?;
    let result = rehearsal().output()?;
    assert!(
        result.status.success(),
        "synthetic receipt command must succeed"
    );
    assert!(!String::from_utf8_lossy(&result.stdout).contains(&synthetic_key));
    assert!(!String::from_utf8_lossy(&result.stderr).contains(&synthetic_key));
    let bytes = std::fs::read(&out)?;
    let receipt: ForecastVerificationReceipt = serde_json::from_slice(&bytes)?;
    assert!(receipt.has_valid_signature_with(&ChainBinding::new("777", "testnet")?));
    let input = ForecastInput {
        forecaster: "synthetic-forecaster".into(),
        station_id: "synthetic-station".into(),
        data_file: data.clone(),
        nonce: "commit-nonce".into(),
        timestamp: 1700000000,
    };
    assert_eq!(receipt.commit_id, input.commit_id()?);
    reveal_body(&input, &manifest, Some(&out))?;
    assert_eq!(std::fs::metadata(&out)?.permissions().mode() & 0o777, 0o600);
    assert!(!rehearsal().output()?.status.success());
    assert_eq!(std::fs::read(&out)?, bytes);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

/// Real binary contract: only explicit --submit writes, and neither key is emitted.
#[cfg(unix)]
#[tokio::test]
async fn bond_grant_request_reads_signs_and_only_submits_explicitly() -> anyhow::Result<()> {
    use libp2p_identity::Keypair;
    use std::os::unix::fs::PermissionsExt;
    use wrfcoin_cli::bond_grant::{GrantRequest, PEER_DOMAIN};
    use wrfcoin_shared_protocol::crypto::signature::{PublicKey, Signature};
    let dir = std::env::temp_dir().join(format!("grant-contract-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let server_seed = hex::encode([41u8; 32]);
    let peer_seed = hex::encode([42u8; 32]);
    let peer = Keypair::ed25519_from_bytes([42u8; 32])?;
    let peer_bytes = peer.to_protobuf_encoding()?;
    let seed_file = dir.join("server.seed");
    let peer_file = dir.join("peer_identity.key");
    std::fs::write(&seed_file, &server_seed)?;
    std::fs::write(&peer_file, &peer_bytes)?;
    for path in [&seed_file, &peer_file] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    let mut node = mockito::Server::new_async().await;
    let mut backend = mockito::Server::new_async().await;
    let info = node.mock("GET", "/network/info")
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({"data": {"chain_id":"888", "network_id":"testnet", "chain_height":102,
            "local_peer_id":peer.public().to_peer_id().to_string()}}).to_string()).expect(2).create_async().await;
    let block = node.mock("GET", "/chain/block/100")
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({"hash":"ab".repeat(32)}).to_string()).expect(2).create_async().await;
    let no_node_write = node.mock("POST", mockito::Matcher::Any).expect(0).create_async().await;
    let command = || {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_wrfcoin"));
        cmd.env("HOME", &dir).env("WRFCOIN_API_URL", "invalid-legacy-url")
            .env("WRFCOIN_DATA_DIR", &dir)
            .args(["--output", "json", "node", "bond", "grant-request", "--owner-address",
                "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "--api-url", &node.url(), "--key-file"])
            .arg(&seed_file);
        cmd
    };
    let no_backend_write = backend.mock("POST", mockito::Matcher::Any).expect(0).create_async().await;
    let out_file = dir.join("request.json");
    let output = command().arg("--out").arg(&out_file).output()?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let request: GrantRequest = serde_json::from_slice(&std::fs::read(&out_file)?)?;
    let printed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(printed["data"], serde_json::to_value(&request)?);
    assert_eq!(request.owner_address, "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(request.height, 100);
    let message = request.message("888", "testnet")?;
    assert!(PublicKey::from_hex(&request.server_key)?.verify(&message, &Signature::from_hex(&request.signature)?));
    assert!(peer.public().verify(&[PEER_DOMAIN, message.as_slice()].concat(), &hex::decode(&request.peer_signature)?));
    no_backend_write.assert_async().await;
    drop(no_backend_write);
    let post = backend.mock("POST", "/api/faucet/bond-grant")
        .match_body(mockito::Matcher::Json(serde_json::to_value(&request)?))
        .with_status(429).with_body("probation pending").expect(1).create_async().await;
    let submitted = command().args(["--submit", &backend.url(), "--p2p-key-file"])
        .arg(&peer_file).output()?;
    assert!(submitted.status.success(), "{}", String::from_utf8_lossy(&submitted.stderr));
    let answer: serde_json::Value = serde_json::from_slice(&submitted.stdout)?;
    assert_eq!(answer["data"]["faucet"]["status"], 429);
    assert_eq!(answer["data"]["faucet"]["body"], "probation pending");
    post.assert_async().await;
    for result in [&output, &submitted] {
        for bytes in [&result.stdout, &result.stderr] {
            let text = String::from_utf8_lossy(bytes);
            for secret in [&server_seed, &peer_seed, &hex::encode(&peer_bytes)] {
                assert!(!text.contains(secret), "private key material leaked");
            }
        }
    }
    // Existing output must refuse before any network request or POST.
    let refused = command().arg("--out").arg(&out_file).args(["--submit", &backend.url()]).output()?;
    assert!(!refused.status.success());
    post.assert_async().await;
    info.assert_async().await;
    block.assert_async().await;
    no_node_write.assert_async().await;
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn bond_grant_refuses_peer_mismatch_and_never_echoes_invalid_keys() -> anyhow::Result<()> {
    use libp2p_identity::Keypair;
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("grant-refusal-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let seed_file = dir.join("server.seed");
    let peer_file = dir.join("peer.key");
    let peer = Keypair::ed25519_from_bytes([44u8; 32])?;
    std::fs::write(&seed_file, hex::encode([43u8; 32]))?;
    std::fs::write(&peer_file, peer.to_protobuf_encoding()?)?;
    for path in [&seed_file, &peer_file] { std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?; }
    let mut node = mockito::Server::new_async().await;
    let info = node.mock("GET", "/network/info").with_header("content-type", "application/json")
        .with_body(serde_json::json!({"chain_id":"888","network_id":"testnet","chain_height":102,
            "local_peer_id":Keypair::ed25519_from_bytes([45u8;32])?.public().to_peer_id().to_string()}).to_string())
        .expect(1).create_async().await;
    let no_block = node.mock("GET", "/chain/block/100").expect(0).create_async().await;
    let no_post = node.mock("POST", mockito::Matcher::Any).expect(0).create_async().await;
    let command = || {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_wrfcoin"));
        cmd.env("HOME", &dir).args(["--output","json","node","bond","grant-request","--owner-address",
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","--api-url",&node.url(),"--submit",&node.url(),"--key-file"])
            .arg(&seed_file).arg("--p2p-key-file").arg(&peer_file);
        cmd
    };
    let output = command().output()?;
    assert!(!output.status.success());
    let text = format!("{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
    assert!(text.contains("peer identity does not match node"));
    for path in [&seed_file, &peer_file] {
        let old = std::fs::read(path)?;
        std::fs::write(path, "private-key-invalid-canary-do-not-echo")?;
        let output = command().output()?;
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private-key-invalid-canary-do-not-echo"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-key-invalid-canary-do-not-echo"));
        std::fs::write(path, old)?;
    }
    info.assert_async().await;
    no_block.assert_async().await;
    no_post.assert_async().await;
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

/// Copied verbatim from wrfcoin/infra#24 at 29d030f44aa6ea157db73a8eba04e3647ea9d4bf,
/// testnet/services/faucet/bond-grant-peer-vector.json. Never regenerate expected bytes here.
#[test]
fn bond_grant_matches_faucet_frozen_dual_signature_vector() -> anyhow::Result<()> {
    use libp2p_identity::Keypair;
    use wrfcoin_cli::bond_grant::{GrantRequest, PEER_DOMAIN};
    use wrfcoin_shared_protocol::crypto::{signature::PrivateKey, signer::EnvKeySigner};
    let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/bond-grant-peer-vector.json"))?;
    let field = |key: &str| fixture[key].as_str().expect("fixed fixture string");
    let mut request: GrantRequest = serde_json::from_value(fixture["request"].clone())?;
    let message = request.message(field("chainId"), field("networkId"))?;
    assert_eq!(hex::encode(&message), field("serverMessageHex"));
    assert_eq!(hex::encode([PEER_DOMAIN, message.as_slice()].concat()), field("peerMessageHex"));
    let peer = Keypair::from_protobuf_encoding(&hex::decode(field("peerPrivateKeyProtobufHex"))?)?;
    assert_eq!(peer.public().to_peer_id().to_string(), request.peer_id);
    assert_eq!(hex::encode(peer.public().try_into_ed25519()?.to_bytes()), field("peerPublicKeyHex"));
    let server = EnvKeySigner::from_private_key(PrivateKey::from_hex(field("testOnlyServerSeedHex"))?, "public test vector");
    request.signature.clear();
    request.peer_signature.clear();
    request.sign(field("chainId"), field("networkId"), &server, &peer)?;
    assert_eq!(serde_json::to_value(&request)?, fixture["request"]);
    Ok(())
}
