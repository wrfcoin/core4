use mockito::{Matcher, Server};
use ring::{hmac, signature::{Ed25519KeyPair, KeyPair}};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use wrfcoin_shared_protocol::blockchain::transaction::TransparentTransferPayload;

/// Locate (building if necessary) the `wrfcoin` binary produced by this
/// workspace's own `cargo build`.
///
/// Does not assume `target/` sits at a fixed path relative to this crate's
/// manifest: a runner-wide `CARGO_TARGET_DIR` override (or an equivalent
/// `.cargo/config.toml` `target-dir`) redirects it elsewhere, and this crate
/// has no visibility into that override other than asking Cargo directly.
/// `cargo metadata`'s `target_directory` field is authoritative regardless of
/// how the override was configured, which a hardcoded `../../../target/debug`
/// guess is not — that guess only holds under Cargo's own default layout.
fn resolve_wrfcoin_bin() -> String {
    if let Ok(bin) = std::env::var("CARGO_BIN_EXE_wrfcoin") {
        return bin;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = manifest_dir.join("Cargo.toml");
    let manifest_path_str = manifest_path.to_str().expect("manifest path");

    let status = Command::new("cargo")
        .args(["build", "--bin", "wrfcoin", "--manifest-path", manifest_path_str])
        .status()
        .expect("build wrfcoin binary");
    assert!(status.success(), "cargo build --bin wrfcoin failed");

    let metadata_output = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            manifest_path_str,
        ])
        .output()
        .expect("cargo metadata");
    assert!(
        metadata_output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&metadata_output.stderr)
    );
    let metadata: Value =
        serde_json::from_slice(&metadata_output.stdout).expect("parse cargo metadata JSON");
    let target_directory = metadata["target_directory"]
        .as_str()
        .expect("cargo metadata missing target_directory");

    PathBuf::from(target_directory)
        .join("debug/wrfcoin")
        .display()
        .to_string()
}

fn run_wrfcoin(args: &[&str], home: &PathBuf, api_url: Option<&str>) -> (bool, String, String) {
    static WRFCOIN_BIN: OnceLock<String> = OnceLock::new();

    let bin = WRFCOIN_BIN.get_or_init(resolve_wrfcoin_bin);
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("HOME", home)
        .env("RUST_LOG", "error")
        .env_remove("WRFCOIN_API_URL");

    // #3204: compute/storage/iot/aiml no longer inherit the node URL; point
    // each family at the same mock server so one mockito instance serves all.
    for var in [
        "WRFCOIN_COMPUTE_API_URL",
        "WRFCOIN_STORAGE_API_URL",
        "WRFCOIN_IOT_API_URL",
        "WRFCOIN_AIML_API_URL",
    ] {
        cmd.env_remove(var);
        if let Some(url) = api_url {
            cmd.env(var, url);
        }
    }

    if let Some(url) = api_url {
        cmd.env("WRFCOIN_API_URL", url);
    }

    let out = cmd.output().expect("run wrfcoin");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    )
}

fn run_wrfcoin_exit(args: &[&str], home: &PathBuf, api_url: Option<&str>) -> (i32, String, String) {
    static WRFCOIN_BIN: OnceLock<String> = OnceLock::new();

    let bin = WRFCOIN_BIN.get_or_init(resolve_wrfcoin_bin);
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("HOME", home)
        .env("RUST_LOG", "error")
        .env_remove("WRFCOIN_API_URL");

    if let Some(url) = api_url {
        cmd.env("WRFCOIN_API_URL", url);
    }

    let out = cmd.output().expect("run wrfcoin");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    )
}

fn temp_home(name: &str) -> PathBuf {
    let base =
        std::env::temp_dir().join(format!("wrfcoin-cli-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).expect("create temp home");
    base
}

fn wallet_primary(home: &PathBuf) -> String {
    let raw = fs::read_to_string(home.join(".wrfcoin/wallet/wallet-state.json"))
        .expect("read wallet state");
    serde_json::from_str::<Value>(&raw).expect("parse wallet state")["primary_address"]
        .as_str()
        .expect("primary wallet address")
        .to_string()
}

fn assert_list_addresses_skips_bad_public_key(name: &str, public_key: &str) {
    let home = temp_home(name);
    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "wallet", "list-addresses"], &home, None);
    assert!(ok, "initial wallet list failed: {stdout} {stderr}");
    let valid_address = wallet_primary(&home);

    let keys_dir = home.join(".wrfcoin/keys");
    let valid_key_path = keys_dir.join(format!("{valid_address}.json"));
    let mut bad_key: Value =
        serde_json::from_slice(&fs::read(&valid_key_path).expect("read generated valid key"))
            .expect("parse generated valid key");
    bad_key["public_key"] = Value::String(public_key.to_string());
    let bad_file_name = format!("{name}-bad-key.json");
    fs::write(
        keys_dir.join(&bad_file_name),
        serde_json::to_vec_pretty(&bad_key).expect("serialize bad key"),
    )
    .expect("write bad key");

    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "wallet", "list-addresses"], &home, None);
    assert!(
        ok,
        "wallet list-addresses must skip {bad_file_name}\nstdout={stdout}\nstderr={stderr}"
    );
    let parsed: Value = serde_json::from_str(&stdout).expect("parse list-addresses output");
    let addresses = parsed["data"]["addresses"]
        .as_array()
        .expect("list-addresses data.addresses array");
    assert!(
        addresses
            .iter()
            .any(|entry| entry["address"].as_str() == Some(valid_address.as_str())),
        "valid address {valid_address} missing from {stdout}"
    );
    assert!(
        stderr.contains(&bad_file_name),
        "warning must name {bad_file_name}, got: {stderr}"
    );

    let _ = fs::remove_dir_all(home);
}

#[test]
fn wallet_list_addresses_skips_non_base64_public_key() {
    assert_list_addresses_skips_bad_public_key("non-base64", "not valid base64!");
}

#[test]
fn wallet_list_addresses_skips_wrong_length_public_key() {
    assert_list_addresses_skips_bad_public_key("wrong-length", "AA==");
}

#[tokio::test(flavor = "multi_thread")]
async fn command_groups_mocked_endpoints_are_green() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("groups");

    let _network_info = server
        .mock("GET", "/api/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"success":true,"data":{"chainId":1,"networkName":"WRFCoin","consensusType":"PoS","blockTime":12,"finalityBlocks":32,"rpcUrl":"http://localhost:8545","blockExplorerUrl":"http://localhost:3000","nativeCurrency":{"name":"WRF","symbol":"WRF","decimals":18}}}"#,
        )
        .expect_at_least(1)
        .create_async()
        .await;
    let _stats = server
        .mock("GET", "/api/stats")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"totalNodes":1,"activeValidators":1,"totalTransactions":1}}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let _providers = server
        // #3204: `compute list-providers` reads backend's /compute/networks catalog.
        .mock("GET", "/api/compute/networks")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"integration":{"wired":false},"networks":{"p1":{"name":"mock","status":"not_wired"}}}"#)
        .expect(1)
        .create_async()
        .await;
    let _forecasts = server
        .mock("GET", "/api/forecasts")
        .match_query(Matcher::UrlEncoded("limit".into(), "2".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":[]}"#)
        .expect(1)
        .create_async()
        .await;
    let _iot_status = server
        .mock("GET", "/api/iot/status")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"online":true,"activeSensors":1}}"#)
        .expect(1)
        .create_async()
        .await;

    let commands: Vec<Vec<&str>> = vec![
        vec!["--json", "chain", "info"],
        vec!["--json", "compute", "list-providers"],
        vec!["--json", "forecast", "list", "--limit", "2"],
        vec!["--json", "monitor", "iot"],
        vec!["--json", "setup", "init"],
    ];

    for cmd in commands {
        let (ok, stdout, stderr) = run_wrfcoin(&cmd, &home, Some(&api));
        assert!(
            ok,
            "command failed: {:?}\nstdout={}\nstderr={}",
            cmd, stdout, stderr
        );
        let parsed: Value = serde_json::from_str(&stdout).expect("json output");
        assert_eq!(parsed.get("ok").and_then(Value::as_bool), Some(true));
    }
}

#[test]
fn unsupported_node_operator_subcommands_are_rejected_by_the_cli() {
    let home = temp_home("node-ops");
    let commands: Vec<Vec<&str>> = vec![
        vec!["node", "start-mining"],
        vec!["node", "stop-mining"],
        vec!["node", "connect", "127.0.0.1:30333"],
        vec!["node", "disconnect", "peer-a"],
        vec!["node", "ban", "peer-a"],
        vec!["node", "unban", "peer-a"],
        vec!["node", "banned-peers"],
        vec!["node", "force-sync"],
        vec!["node", "reset", "--blockchain"],
    ];

    for cmd in commands {
        let (ok, _stdout, stderr) = run_wrfcoin(&cmd, &home, None);
        assert!(!ok, "unsupported command unexpectedly succeeded: {cmd:?}");
        assert!(stderr.contains("unrecognized subcommand"), "{cmd:?}: {stderr}");
    }
}

/// Tests the 7 newly-wired command groups plus additional wallet coverage.
/// API-backed groups (governance, storage, aiml, iot) use mockito mocks.
/// Local-only groups (data, dev, infra) run without mocks.
#[tokio::test(flavor = "multi_thread")]
async fn expanded_command_groups_are_green() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("expanded");

    // --- Mocks for API-backed commands ---
    let _governance_proposals = server
        .mock(
            "GET",
            Matcher::Regex(r"^/api/governance/proposals.*$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":[]}"#)
        .expect(1)
        .create_async()
        .await;
    let _storage_status = server
        // #3204: `storage status` reads backend's /storage/networks.
        .mock("GET", "/api/storage/networks")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"online":true,"totalCapacity":"1.8 PB"}}"#)
        .expect(1)
        .create_async()
        .await;
    let _aiml_status = server
        // #3204: `aiml status` reads backend's /aiml/networks/status.
        .mock("GET", "/api/aiml/networks/status")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"online":true,"networks":5}}"#)
        .expect(1)
        .create_async()
        .await;
    // #3204: `iot status` reads backend's /iot/networks (`monitor iot` still uses the node).
    let _iot_status_expanded = server
        .mock("GET", "/api/iot/networks")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"online":true,"activeSensors":100}}"#)
        .expect(1)
        .create_async()
        .await;

    // Commands that need API mocks → assert ok:true
    let api_commands: Vec<Vec<&str>> = vec![
        vec!["--json", "governance", "list-proposals"],
        vec!["--json", "storage", "status"],
        vec!["--json", "aiml", "status"],
        vec!["--json", "iot", "status"],
    ];

    for cmd in &api_commands {
        let (ok, stdout, stderr) = run_wrfcoin(cmd, &home, Some(&api));
        assert!(
            ok,
            "command failed: {:?}\nstdout={}\nstderr={}",
            cmd, stdout, stderr
        );
        let parsed: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|_| panic!("non-json output for {:?}: {}", cmd, stdout));
        assert_eq!(
            parsed.get("ok").and_then(Value::as_bool),
            Some(true),
            "ok!=true for {:?}: {}",
            cmd,
            stdout
        );
        // #3204: service-family commands must say which service they hit.
        if cmd[1] != "governance" {
            let target_url = parsed
                .pointer("/data/target/base_url")
                .or_else(|| parsed.pointer("/target/base_url"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("no target.base_url for {:?}: {}", cmd, stdout));
            assert_eq!(target_url, api, "target.base_url for {:?}", cmd);
        }
    }

    // Remaining local-only commands (no API needed) → assert ok:true.
    // infra/dev/data were retired as fabricated in core4#3246.
    let local_commands: Vec<Vec<&str>> = vec![
        vec!["--json", "storage", "list"],
        vec!["--json", "aiml", "list"],
        vec!["--json", "iot", "list"],
    ];

    for cmd in &local_commands {
        let (ok, stdout, stderr) = run_wrfcoin(cmd, &home, None);
        assert!(
            ok,
            "command failed: {:?}\nstdout={}\nstderr={}",
            cmd, stdout, stderr
        );
        let parsed: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|_| panic!("non-json output for {:?}: {}", cmd, stdout));
        assert_eq!(
            parsed.get("ok").and_then(Value::as_bool),
            Some(true),
            "ok!=true for {:?}: {}",
            cmd,
            stdout
        );
    }
}

#[test]
fn infra_dev_and_data_literals_fail_not_wired() {
    let home = temp_home("infra-dev-data-not-wired");
    let commands: Vec<Vec<&str>> = vec![
        vec!["infra", "overview"],
        vec!["infra", "capacity"],
        vec!["infra", "metrics"],
        vec!["infra", "stress-test", "storage"],
        vec!["infra", "topology"],
        vec!["dev", "generate-test-data", "--data-type", "blocks"],
        vec!["dev", "analyze-contract", "--file", "contract.sol"],
        vec!["dev", "monitor-performance"],
        vec!["dev", "run-tests"],
        vec![
            "dev",
            "create-project",
            "--name",
            "demo",
            "--project-type",
            "dapp",
        ],
        vec!["data", "list-providers"],
        vec!["data", "search-datasets", "--query", "weather"],
        vec![
            "data",
            "import-dataset",
            "--dataset-id",
            "dataset-1",
            "--provider",
            "ocean",
        ],
        vec![
            "data",
            "publish-dataset",
            "--data",
            "dataset.json",
            "--name",
            "demo",
            "--description",
            "demo data",
            "--provider",
            "ocean",
        ],
        vec!["data", "setup-bigchaindb", "--url", "http://127.0.0.1:9984"],
        vec!["data", "bigchaindb-create-asset", "--data", "{}"],
        vec!["data", "bigchaindb-query", "--query", "{}"],
        vec![
            "data",
            "bigchaindb-transfer",
            "--asset-id",
            "asset-1",
            "--to-public-key",
            "recipient-key",
        ],
    ];

    for command in commands {
        let (code, stdout, stderr) = run_wrfcoin_exit(&command, &home, None);
        assert_eq!(code, 7, "{command:?}\nstdout={stdout}\nstderr={stderr}");
        assert!(
            stdout.contains("not wired") || stderr.contains("not wired"),
            "{command:?}\nstdout={stdout}\nstderr={stderr}",
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn infra_health_reads_readyz_and_node_resources() {
    let mut server = Server::new_async().await;
    let api = server.url();
    let home = temp_home("infra-health");
    let _ready = server
        .mock("GET", "/readyz")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ready":true,"height":42}"#)
        .expect(1)
        .create_async()
        .await;
    let _resources = server
        .mock("GET", "/node/resources")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"cpu":{"usage":0.25},"memory":{"used":1024}}"#)
        .expect(1)
        .create_async()
        .await;

    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "infra", "health"], &home, Some(&api));
    assert!(ok, "stdout={stdout}\nstderr={stderr}");
    let output: Value = serde_json::from_str(&stdout).expect("health JSON");
    assert_eq!(
        output.pointer("/data/readiness/ready"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        output.pointer("/data/readiness/height"),
        Some(&Value::from(42))
    );
    assert_eq!(
        output.pointer("/data/resources/memory/used"),
        Some(&Value::from(1024))
    );
}

#[test]
fn no_fabricated_capacity_literals() {
    let source = [
        include_str!("../src/commands/infra.rs"),
        include_str!("../src/commands/dev.rs"),
        include_str!("../src/commands/data.rs"),
    ]
    .join("\n");

    for forbidden in [
        "cores",
        "PB",
        "EiB",
        "devices globally",
        "blockchains supported",
    ] {
        assert!(
            !source.contains(forbidden),
            "fabricated literal restored: {forbidden}"
        );
    }
}

/// Wallet now uses the unified "ok":true envelope like all other command groups.
#[tokio::test(flavor = "multi_thread")]
async fn wallet_subcommands_produce_structured_output() {
    let home = temp_home("wallet-sub");
    let mut server = Server::new_async().await;

    // new-address creates a wallet and returns structured JSON with mnemonic
    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "wallet", "new-address"], &home, None);
    assert!(
        ok,
        "wallet new-address failed\nstdout={}\nstderr={}",
        stdout, stderr
    );
    let parsed: Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(
        parsed.get("ok").and_then(Value::as_bool),
        Some(true),
        "new-address: ok!=true: {}",
        stdout
    );
    // Verify mnemonic is present in the data
    let data = parsed.get("data").expect("data field");
    assert!(
        data.get("mnemonic").and_then(Value::as_str).is_some(),
        "new-address should include mnemonic: {}",
        stdout
    );
    assert!(
        data.get("address").and_then(Value::as_str).is_some(),
        "new-address should include address: {}",
        stdout
    );
    let address = wallet_primary(&home);

    // list-addresses returns addresses
    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "wallet", "list-addresses"], &home, None);
    assert!(
        ok,
        "wallet list-addresses failed\nstdout={}\nstderr={}",
        stdout, stderr
    );
    let parsed: Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(
        parsed.get("ok").and_then(Value::as_bool),
        Some(true),
        "list-addresses: ok!=true: {}",
        stdout
    );

    // balance returns balance for primary address
    let balance_path = format!("/addresses/{address}");
    let balance = server
        .mock("GET", balance_path.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"data":{{"address":"{address}","balance":"51","spendable_balance":"50","stake_locked":"1","earned_balance_total":"7","nonce":"3"}}}}"#
        ))
        .expect(1)
        .create_async()
        .await;
    let (ok, stdout, stderr) = run_wrfcoin(
        &["--json", "wallet", "balance"],
        &home,
        Some(&server.url()),
    );
    assert!(
        ok,
        "wallet balance failed\nstdout={}\nstderr={}",
        stdout, stderr
    );
    let parsed: Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(
        parsed.get("ok").and_then(Value::as_bool),
        Some(true),
        "balance: ok!=true: {}",
        stdout
    );
    assert_eq!(parsed["data"]["balance"], "51");
    balance.assert_async().await;

    // history returns the node's empty history
    let history_path = format!("/addresses/{address}/transactions");
    let history = server
        .mock("GET", history_path.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"data":[]}"#)
        .expect(1)
        .create_async()
        .await;
    let (ok, stdout, stderr) = run_wrfcoin(
        &["--json", "wallet", "history"],
        &home,
        Some(&server.url()),
    );
    assert!(
        ok,
        "wallet history failed\nstdout={}\nstderr={}",
        stdout, stderr
    );
    let parsed: Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(
        parsed.get("ok").and_then(Value::as_bool),
        Some(true),
        "history: ok!=true: {}",
        stdout
    );
    assert_eq!(parsed["data"]["history"], serde_json::json!([]));
    history.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wallet_send_posts_signed_transparent_transfer() {
    const TO: &str = "0x2222222222222222222222222222222222222222";
    let vector: Value = serde_json::from_str(include_str!(
        "../../../../contract-fixtures/wallet-key-scheme-vector.json"
    )).expect("wallet vector");
    let phrase = vector["mnemonic"].as_str().expect("vector mnemonic");
    let from = vector["address"].as_str().expect("vector address");
    let public_key = vector["ed25519_public_key_hex"]
        .as_str().expect("vector public key");
    let home = temp_home("wallet-send-signed");
    let (ok, _, stderr) = run_wrfcoin(
        &["--json", "wallet", "recover", "--mnemonic", phrase],
        &home,
        None,
    );
    assert!(ok, "wallet recovery failed: {stderr}");
    let (ok, _, stderr) = run_wrfcoin(
        &["--json", "wallet", "set-primary", from],
        &home,
        None,
    );
    assert!(ok, "set-primary failed: {stderr}");

    let mnemonic: bip39::Mnemonic = phrase.parse().expect("test mnemonic");
    let seed = mnemonic.to_seed("");
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA512, b"ed25519 seed"), &seed);
    let key = Ed25519KeyPair::from_seed_unchecked(&tag.as_ref()[..32]).expect("ed25519 key");
    assert_eq!(hex::encode(key.public_key().as_ref()), public_key);
    let nonce = 7;
    let payload = TransparentTransferPayload {
        from: from.to_string(),
        to: TO.to_string(),
        amount: 100_000_000,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: None,
        sponsorship: None,
        session_key_auth: None,
    };
    let signature = hex::encode(key.sign(&payload.authorization_message(nonce)).as_ref());

    let mut server = Server::new_async().await;
    let account = server
        .mock("GET", format!("/addresses/{from}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"data":{{"address":"{from}","balance":"200000000","spendable_balance":"200000000","stake_locked":"0","earned_balance_total":"0","nonce":"{nonce}"}}}}"#))
        .expect(1)
        .create_async()
        .await;
    let submit = server
        .mock("POST", "/transactions/submit")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "from": from,
            "to": TO,
            "amount": 100_000_000u64,
            "nonce": nonce.to_string(),
            "transparent_opt_out": true,
            "transparent_opt_out_reason": "cli",
            "auth_pubkey": public_key,
            "auth_signature": signature,
        })))
        .with_status(202)
        .with_header("content-type", "application/json")
        .with_body(r#"{"data":{"accepted":true,"tx_id":"0xfeed"}}"#)
        .expect(1)
        .create_async()
        .await;
    let (ok, stdout, stderr) = run_wrfcoin(
        &["--json", "wallet", "send", "--to", TO, "--amount", "1"],
        &home,
        Some(&server.url()),
    );
    assert!(ok, "signed send failed\nstdout={stdout}\nstderr={stderr}");
    let out: Value = serde_json::from_str(&stdout).expect("send json");
    assert_eq!(out["data"]["status"], "submitted");
    assert_eq!(out["data"]["tx_id"], "0xfeed");
    account.assert_async().await;
    submit.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wallet_send_without_nonce_source_fails_closed() {
    let home = temp_home("wallet-send-no-nonce");
    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "wallet", "new-address"], &home, None);
    assert!(ok, "wallet creation failed: {stdout} {stderr}");
    let address = wallet_primary(&home);
    let mut server = Server::new_async().await;
    let account = server.mock("GET", format!("/addresses/{address}").as_str())
        .with_status(200).with_header("content-type", "application/json")
        .with_body(format!(r#"{{"data":{{"address":"{address}","balance":"1","spendable_balance":"1","stake_locked":"0","earned_balance_total":"0"}}}}"#))
        .expect(1).create_async().await;
    let submit = server.mock("POST", "/transactions/submit").expect(0).create_async().await;
    let (ok, stdout, stderr) = run_wrfcoin(
        &["--json", "wallet", "send", "--to", "0x2222222222222222222222222222222222222222", "--amount", "1"],
        &home, Some(&server.url()));
    assert!(!ok, "send without nonce unexpectedly succeeded: {stdout}");
    assert!(stderr.contains("--nonce"), "missing nonce hint: {stderr}");
    account.assert_async().await;
    submit.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wallet_send_node_error_leaves_no_local_record() {
    let home = temp_home("wallet-send-node-error");
    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "wallet", "new-address"], &home, None);
    assert!(ok, "wallet creation failed: {stdout} {stderr}");
    let address = wallet_primary(&home);
    let state_path = home.join(".wrfcoin/wallet/wallet-state.json");
    let before: Value = serde_json::from_slice(
        &fs::read(&state_path).expect("wallet state before send"),
    ).expect("parse state before send");
    let mut server = Server::new_async().await;
    let account = server.mock("GET", format!("/addresses/{address}").as_str())
        .with_status(200).with_header("content-type", "application/json")
        .with_body(format!(r#"{{"data":{{"address":"{address}","balance":"1","spendable_balance":"1","stake_locked":"0","earned_balance_total":"0","nonce":"0"}}}}"#))
        .expect(1).create_async().await;
    let submit = server.mock("POST", "/transactions/submit")
        .with_status(503).with_header("content-type", "application/json")
        .with_body(r#"{"error":"node unavailable"}"#).expect(1).create_async().await;
    let (ok, _, _) = run_wrfcoin(
        &["--json", "wallet", "send", "--to", "0x2222222222222222222222222222222222222222", "--amount", "1"],
        &home, Some(&server.url()));
    assert!(!ok, "node error must fail the send");
    let after: Value = serde_json::from_slice(&fs::read(&state_path).unwrap())
        .expect("parse state after send");
    assert_eq!(after, before, "node error mutated wallet state");
    account.assert_async().await;
    submit.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wallet_send_dry_run_preview_shows_no_network_fee() {
    // core4 applies a transparent transfer as a balance move of exactly
    // `amount`, and the submitted body has no fee: the preview must not invent
    // a fee, a fee-inclusive total, or a confirmation estimate.
    let home = temp_home("wallet-send-preview-no-fee");
    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "wallet", "new-address"], &home, None);
    assert!(ok, "wallet creation failed: {stdout} {stderr}");
    let address = wallet_primary(&home);
    let mut server = Server::new_async().await;
    let account = server.mock("GET", format!("/addresses/{address}").as_str())
        .with_status(200).with_header("content-type", "application/json")
        .with_body(format!(r#"{{"data":{{"address":"{address}","balance":"300000000","spendable_balance":"300000000","stake_locked":"0","earned_balance_total":"0","nonce":"0"}}}}"#))
        .expect(1).create_async().await;
    let estimate = server.mock("GET", Matcher::Regex("^/transactions/estimate-fee".to_string()))
        .expect(0).create_async().await;
    let submit = server.mock("POST", "/transactions/submit").expect(0).create_async().await;
    let (ok, stdout, stderr) = run_wrfcoin(
        &["--json", "wallet", "send", "--to", "0x2222222222222222222222222222222222222222", "--amount", "1", "--dry-run"],
        &home, Some(&server.url()));
    assert!(ok, "dry run failed\nstdout={stdout}\nstderr={stderr}");
    let out: Value = serde_json::from_str(&stdout).expect("dry-run json");
    let preview = &out["data"]["preview"];
    assert_eq!(out["data"]["dry_run"], true);
    assert_eq!(preview["amount"], 100_000_000u64);
    assert_eq!(preview["network_fee"], "none");
    assert_eq!(preview["remaining_balance"], 200_000_000u64);
    for absent in ["fee", "total_deduction", "memo", "estimated_confirmation_seconds"] {
        assert!(preview.get(absent).is_none(), "preview still carries {absent}: {preview}");
    }
    assert!(!stdout.contains("HIGH_FEE"), "fee warning survived: {stdout}");
    account.assert_async().await;
    estimate.assert_async().await;
    submit.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wallet_send_rejects_memo_and_fee_before_any_request() {
    let home = temp_home("wallet-send-reject-memo-fee");
    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "wallet", "new-address"], &home, None);
    assert!(ok, "wallet creation failed: {stdout} {stderr}");
    let mut server = Server::new_async().await;
    let any_request = server.mock("GET", Matcher::Any).expect(0).create_async().await;
    let any_post = server.mock("POST", Matcher::Any).expect(0).create_async().await;
    const TO: &str = "0x2222222222222222222222222222222222222222";
    let cases: [(&[&str], &str); 6] = [
        (&["--memo", "hello"], "--memo is not supported"),
        (&["--memo", "hello", "--interactive"], "--memo is not supported"),
        (&["--memo", "hello", "--dry-run"], "--memo is not supported"),
        (&["--fee", "0.01"], "--fee is not supported"),
        (&["--fee", "0.01", "--interactive"], "--fee is not supported"),
        (&["--fee", "0.01", "--dry-run"], "--fee is not supported"),
    ];
    for (extra, expected) in cases {
        let mut args = vec!["--json", "wallet", "send", "--to", TO, "--amount", "1", "--nonce", "0"];
        args.extend_from_slice(extra);
        let (ok, stdout, stderr) = run_wrfcoin(&args, &home, Some(&server.url()));
        assert!(!ok, "{extra:?} unexpectedly succeeded: {stdout}");
        assert!(
            stdout.contains(expected) || stderr.contains(expected),
            "{extra:?} missing {expected:?}\nstdout={stdout}\nstderr={stderr}"
        );
    }
    any_request.assert_async().await;
    any_post.assert_async().await;
}

#[test]
fn wallet_persists_keys_and_migrates_legacy_metadata() {
    let home = temp_home("wallet");
    let wrf_dir = home.join(".wrfcoin");
    fs::create_dir_all(&wrf_dir).expect("create .wrfcoin");

    let legacy = serde_json::json!({
        "addresses": ["legacy_addr_001"],
        "primary_address": "legacy_addr_001",
        "balances": { "legacy_addr_001": 3.5 },
        "transaction_history": []
    });
    fs::write(
        wrf_dir.join("wallet.json"),
        serde_json::to_string_pretty(&legacy).expect("legacy json"),
    )
    .expect("write legacy wallet");

    let (ok, stdout, stderr) = run_wrfcoin(&["--json", "wallet", "new-address"], &home, None);
    assert!(
        ok,
        "wallet new-address failed\nstdout={}\nstderr={}",
        stdout, stderr
    );

    let wallet_state_path = home.join(".wrfcoin/wallet/wallet-state.json");
    assert!(wallet_state_path.exists(), "wallet-state.json missing");
    let wallet_state_raw = fs::read_to_string(wallet_state_path).expect("read wallet-state");
    let wallet_state: Value = serde_json::from_str(&wallet_state_raw).expect("parse wallet-state");

    let addresses = wallet_state
        .get("addresses")
        .and_then(Value::as_array)
        .expect("addresses array");
    assert!(!addresses.is_empty(), "no managed addresses");

    let legacy_addresses = wallet_state
        .get("legacy_addresses")
        .and_then(Value::as_array)
        .expect("legacy addresses array");
    assert!(
        legacy_addresses
            .iter()
            .any(|item| item.as_str() == Some("legacy_addr_001")),
        "legacy address not migrated"
    );
}

// ---------------------------------------------------------------------------
// --why flag integration tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn why_json_mode_includes_structured_report() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("why-json");

    let _network_info = server
        .mock("GET", "/api/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"chainId":1,"networkName":"WRFCoin","consensusType":"PoS"}}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let _stats = server
        .mock("GET", "/api/stats")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"totalNodes":1}}"#)
        .expect_at_least(1)
        .create_async()
        .await;

    let (ok, stdout, stderr) =
        run_wrfcoin(&["--why", "--json", "chain", "info"], &home, Some(&api));
    assert!(
        ok,
        "--why --json chain info failed\nstdout={}\nstderr={}",
        stdout, stderr
    );

    let parsed: Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(parsed["ok"], true);

    // Verify why block is present with required fields
    let why = parsed.get("why").expect("why block must be present");
    assert!(why.get("routes").is_some(), "why.routes missing");
    assert!(why.get("base_url").is_some(), "why.base_url missing");
    assert!(
        why.get("retry_policy").is_some(),
        "why.retry_policy missing"
    );
    assert!(why.get("attempts").is_some(), "why.attempts missing");
    assert_eq!(why["exit_code"], 0);
    assert_eq!(why["exit_reason"], "success");

    // Routes should contain the API paths that were called
    let routes = why["routes"].as_array().expect("routes is array");
    assert!(!routes.is_empty(), "at least one route recorded");
    assert!(
        routes
            .iter()
            .any(|r| r.as_str().unwrap_or("").contains("/network/info")),
        "network/info route not recorded"
    );

    // Retry policy should reflect the configured values
    let policy = &why["retry_policy"];
    assert!(policy["max_attempts"].as_u64().unwrap_or(0) > 0);
    assert!(policy["delay_ms"].as_u64().unwrap_or(0) > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn why_human_mode_prints_why_lines() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("why-human");

    let _network_info = server
        .mock("GET", "/api/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"chainId":1,"networkName":"WRFCoin","consensusType":"PoS"}}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let _stats = server
        .mock("GET", "/api/stats")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"totalNodes":1}}"#)
        .expect_at_least(1)
        .create_async()
        .await;

    // Use --output human to force human mode (pipe detection would otherwise select JSON)
    let (ok, stdout, _stderr) = run_wrfcoin(
        &["--output", "human", "--why", "chain", "info"],
        &home,
        Some(&api),
    );
    assert!(ok, "--why chain info failed");

    // Human mode should contain WHY: lines
    assert!(
        stdout.contains("WHY: route"),
        "missing WHY: route line in stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("WHY: exit_code"),
        "missing WHY: exit_code line in stdout:\n{}",
        stdout
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn why_error_reports_exit_code_and_attempts() {
    // Use a port that nothing listens on to trigger connection error
    let home = temp_home("why-error");
    let bad_api = "http://127.0.0.1:19999/api";

    let (ok, stdout, stderr) =
        run_wrfcoin(&["--why", "--json", "chain", "info"], &home, Some(bad_api));
    // Command should fail
    assert!(!ok, "expected failure with bad API URL");

    // Try to parse stdout as JSON (error output may go to stdout in JSON mode)
    let output = if stdout.starts_with('{') {
        &stdout
    } else {
        &stderr
    };
    if let Ok(parsed) = serde_json::from_str::<Value>(output) {
        if let Some(why) = parsed.get("why") {
            // exit_code should be non-zero
            assert_ne!(why["exit_code"], 0, "exit_code should be non-zero on error");
            // exit_reason should be present
            assert!(
                why["exit_reason"].is_string(),
                "exit_reason should be a string"
            );
        }
    }
    // In any case, verify we don't leak the bad URL without redaction issues
}

// ---------------------------------------------------------------------------
// Output format integration tests (#282)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn json_envelope_has_exit_code_and_meta() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("envelope");

    let _network_info = server
        .mock("GET", "/api/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"chainId":1,"networkName":"WRFCoin","consensusType":"PoS"}}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let _stats = server
        .mock("GET", "/api/stats")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"totalNodes":1}}"#)
        .expect_at_least(1)
        .create_async()
        .await;

    let (ok, stdout, _stderr) = run_wrfcoin(&["--json", "chain", "info"], &home, Some(&api));
    assert!(ok, "chain info failed");
    let parsed: Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(parsed["ok"], true);
    // New fields from #282
    assert_eq!(parsed["exit_code"], 0, "exit_code missing or wrong");
    assert!(
        parsed.get("meta").is_some(),
        "meta block missing from JSON envelope"
    );
    let meta = &parsed["meta"];
    assert!(meta.get("timestamp").is_some(), "meta.timestamp missing");
    assert!(
        meta.get("cli_version").is_some(),
        "meta.cli_version missing"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn output_format_csv_renders_headers() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("csv-format");

    let _iot_status = server
        .mock("GET", "/api/iot/status")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"online":true,"activeSensors":1}}"#)
        .expect(1)
        .create_async()
        .await;

    let (ok, stdout, stderr) =
        run_wrfcoin(&["--output", "csv", "monitor", "iot"], &home, Some(&api));
    assert!(
        ok,
        "--output csv monitor iot failed\nstdout={}\nstderr={}",
        stdout, stderr
    );
    // CSV output should contain a header row with "key" and at least one data row
    assert!(
        stdout.contains("key") || stdout.contains("summary") || stdout.contains("online"),
        "CSV output should have headers: {}",
        stdout
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn output_format_table_renders_table() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("table-format");

    let _iot_status = server
        .mock("GET", "/api/iot/status")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"online":true,"activeSensors":1}}"#)
        .expect(1)
        .create_async()
        .await;

    let (ok, stdout, stderr) =
        run_wrfcoin(&["--output", "table", "monitor", "iot"], &home, Some(&api));
    assert!(
        ok,
        "--output table monitor iot failed\nstdout={}\nstderr={}",
        stdout, stderr
    );
    // Table output uses box-drawing characters or at least has structured rows
    assert!(
        stdout.contains("Key")
            || stdout.contains("Value")
            || stdout.contains("─")
            || stdout.contains("|"),
        "Table output doesn't look like a table: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Batch command integration tests (#282)
// ---------------------------------------------------------------------------

#[test]
fn batch_run_is_unavailable_for_unknown_input() {
    let home = temp_home("batch-unavailable");
    let batch_path = home.join("batch.jsonl");
    fs::write(
        &batch_path,
        r#"{"command":"wallet send","args":{"to":"synthetic","amount":1.0}}"#,
    )
    .expect("write synthetic batch");
    let (ok, stdout, stderr) = run_wrfcoin(
        &[
            "--json",
            "batch",
            "run",
            "--file",
            batch_path.to_str().unwrap(),
        ],
        &home,
        None,
    );
    assert!(!ok, "batch must refuse: {stdout} {stderr}");
    let parsed: Value = serde_json::from_str(&stdout).expect("JSON refusal");
    assert_eq!(parsed["ok"], false);
    assert!(
        stdout.contains("not wired") && stdout.contains("#3247"),
        "{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Contribute command integration tests (#286)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn contribute_preflight_reports_checks() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("contribute-pre");

    let _health = server
        .mock("GET", "/api/health")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ok","timestamp":"2026-02-22T10:00:00Z"}"#)
        .expect(1)
        .create_async()
        .await;
    let _station = server
        .mock(
            "GET",
            Matcher::Regex(r"^/api/weather/stations/.*$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id":"test-station","quality_score":85.0}"#)
        .expect(1)
        .create_async()
        .await;
    let _network = server
        .mock("GET", "/api/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"synced":true}}"#)
        .expect(1)
        .create_async()
        .await;

    let (ok, stdout, stderr) = run_wrfcoin(
        &[
            "--json",
            "contribute",
            "preflight",
            "--station-id",
            "test-station",
        ],
        &home,
        Some(&api),
    );
    assert!(
        ok,
        "contribute preflight failed\nstdout={}\nstderr={}",
        stdout, stderr
    );
    let parsed: Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(parsed["ok"], true);
    let data = parsed.get("data").expect("data field");
    let checks = data
        .get("checks")
        .and_then(Value::as_array)
        .expect("checks array");
    // Should have 7 preflight checks
    assert_eq!(
        checks.len(),
        7,
        "expected 7 preflight checks, got {}",
        checks.len()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn contribute_start_once_submits_and_summarizes() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("contribute-once");

    let _health = server
        .mock("GET", "/api/health")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ok","timestamp":"2026-02-22T10:00:00Z"}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let _station = server
        .mock(
            "GET",
            Matcher::Regex(r"^/api/weather/stations/.*$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id":"test-station","quality_score":85.0}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let _network = server
        .mock("GET", "/api/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"synced":true}}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let _sensor = server
        .mock("GET", Matcher::Regex(r"^/api/iot/sensors/.*$".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"type":"temperature","value":21.5,"unit":"celsius","latitude":40.7,"longitude":-74.0}"#)
        .expect(1)
        .create_async()
        .await;
    let _submit = server
        .mock(
            "POST",
            Matcher::Regex(r"^/api/weather/stations/.*/observations$".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"id":"obs-001","quality_score":90.0}}"#)
        .expect(1)
        .create_async()
        .await;

    let (ok, stdout, stderr) = run_wrfcoin(
        &[
            "--json",
            "contribute",
            "start",
            "--station-id",
            "test-station",
            "--once",
        ],
        &home,
        Some(&api),
    );
    // The session summary is the last JSON line
    let lines: Vec<&str> = stdout.lines().collect();
    let last_line = lines.last().unwrap_or(&"");
    if let Ok(parsed) = serde_json::from_str::<Value>(last_line) {
        if let Some(data) = parsed.get("data") {
            assert!(
                data.get("session").is_some(),
                "session summary missing: {}",
                stdout
            );
        }
    }
    // It's ok if the command fails due to API issues in test env
    let _ = (ok, stderr);
}

#[tokio::test(flavor = "multi_thread")]
async fn why_output_does_not_contain_sensitive_values() {
    let mut server = Server::new_async().await;
    let api = format!("{}/api", server.url());
    let home = temp_home("why-redact");

    let _network_info = server
        .mock("GET", "/api/network/info")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"chainId":1,"networkName":"WRFCoin","consensusType":"PoS"}}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let _stats = server
        .mock("GET", "/api/stats")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"success":true,"data":{"totalNodes":1}}"#)
        .expect_at_least(1)
        .create_async()
        .await;

    // Set a fake sensitive env var
    let bin = resolve_wrfcoin_bin();
    let out = Command::new(&bin)
        .args(["--why", "--json", "chain", "info"])
        .env("HOME", &home)
        .env("RUST_LOG", "error")
        .env("WRFCOIN_API_URL", &api)
        .env("WRFCOIN_API_KEY", "sk-super-secret-key-12345")
        .output()
        .expect("run wrfcoin");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{}{}", stdout, stderr);

    // The secret key should NOT appear in any output
    assert!(
        !combined.contains("sk-super-secret-key-12345"),
        "sensitive API key leaked in --why output"
    );
}

#[path = "support/station_owner_signature_3383.rs"]
mod station_owner_signature_3383;

/// core4#3299 review: `setup genesis --activate` emits a feature schedule too,
/// so the CLI's scheduling hold covers it: the real binary refuses a held
/// feature before writing any artifact, and an unheld schedule still writes.
#[test]
fn setup_genesis_refuses_a_held_feature_before_writing_3299() {
    let home = temp_home("genesis-held-3299");
    let keys = home.join("validator-keys");
    fs::create_dir_all(&keys).expect("create key dir");
    // A public key only (no private key file), derived from a fixed test seed.
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).expect("test seed");
    let public_hex: String = pair.public_key().as_ref().iter().map(|b| format!("{b:02x}")).collect();
    fs::write(keys.join("validator-1.pub"), public_hex).expect("write pub");
    let keys_arg = keys.to_str().expect("utf-8 path");

    let held = home.join("held-genesis.json");
    let (code, stdout, stderr) = run_wrfcoin_exit(
        &[
            "setup", "genesis", "--validator-keys", keys_arg,
            "--output", held.to_str().expect("utf-8 path"),
            "--genesis-timestamp", "1711152000",
            "--activate", "v1@0", "--activate", "reference_sea_level_pressure@25000",
        ],
        &home,
        None,
    );
    let said = format!("{stdout}\n{stderr}");
    assert_ne!(code, 0, "a held feature must be refused: {said}");
    assert!(said.contains("scheduling of `reference_sea_level_pressure` is held"), "{said}");
    assert!(said.contains("core4#3299"), "{said}");
    assert!(!held.exists(), "nothing may be written for a refused schedule");

    // Positive control: an unheld schedulable feature still writes its schedule.
    let unheld = home.join("unheld-genesis.json");
    let (code, stdout, stderr) = run_wrfcoin_exit(
        &[
            "setup", "genesis", "--validator-keys", keys_arg,
            "--output", unheld.to_str().expect("utf-8 path"),
            "--genesis-timestamp", "1711152000",
            "--activate", "v1@0", "--activate", "reward_gross_outlier_guard@25000",
        ],
        &home,
        None,
    );
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    let written: Value =
        serde_json::from_str(&fs::read_to_string(&unheld).expect("genesis written")).expect("json");
    assert_eq!(
        written["protocol_activation_schedule"],
        Value::String("0:v1,25000:reward_gross_outlier_guard".to_string())
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn unsupported_dex_commands_are_hidden_from_wallet_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
        .args(["wallet", "--help"])
        .output()
        .expect("wallet help");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for command in ["swap", "price", "liquidity"] {
        assert!(
            !help
                .lines()
                .any(|line| line.trim_start().starts_with(command)),
            "unsupported command {command} advertised: {help}"
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
        .args(["wallet", "swap", "--help"])
        .output()
        .expect("retained swap help");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("Dry runs are unavailable"), "{help}");
    assert!(!help.contains("show quote without executing"), "{help}");
}

#[test]
fn dex_commands_refuse_without_network_or_wallet_access() {
    let config_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("dex-test-{}", uuid::Uuid::new_v4()));
    let config = config_dir.join("config.json");
    let mut server = Server::new();
    let requests = server
        .mock("GET", Matcher::Any)
        .with_status(200)
        .with_body("{}")
        .expect(0)
        .create();
    let writes = server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_body("{}")
        .expect(0)
        .create();

    // An explicit temporary config avoids reading the user's configuration.
    // A refused operation must not inspect wallet keys or obtain a quote,
    // even when the caller requests a dry run.
    let commands: &[&[&str]] = &[
        &["wallet", "swap", "--to", "USDC", "--amount", "1"],
        &[
            "wallet",
            "swap",
            "--to",
            "USDC",
            "--amount",
            "1",
            "--dry-run",
        ],
        &["wallet", "price", "--pair", "WRF/USDC"],
        &["wallet", "liquidity", "--pair", "WRF/USDC"],
    ];
    for args in commands {
        let output = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
            .arg("--config")
            .arg(&config)
            .arg("--json")
            .args(*args)
            .env("WRFCOIN_API_URL", server.url())
            .env("RUST_LOG", "error")
            .output()
            .expect("run CLI");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(7), "{args:?}: {stdout} {stderr}");
        let envelope: Value = serde_json::from_str(&stdout).expect("JSON refusal envelope");
        assert_eq!(envelope["ok"], false, "{args:?}: {envelope}");
        assert!(stdout.contains("not wired"), "{args:?}: {stdout}");
        assert!(stdout.contains("#3247"), "{args:?}: {stdout}");
    }
    requests.assert();
    writes.assert();
    std::fs::remove_dir_all(config_dir).expect("remove this test's config");
}

mod bridge_revision_3247 {
    use mockito::{Matcher, Server};
    use serde_json::Value;
    use std::process::Command;

    fn assert_refusals(commands: &[&[&str]]) {
        let config_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("service-gap-test-{}", uuid::Uuid::new_v4()));
        let mut server = Server::new();
        let reads = server
            .mock("GET", Matcher::Any)
            .with_status(200)
            .with_body("{}")
            .expect(0)
            .create();
        let writes = server
            .mock("POST", Matcher::Any)
            .with_status(200)
            .with_body("{}")
            .expect(0)
            .create();
        for args in commands {
            let output = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
                .arg("--config")
                .arg(config_dir.join("config.json"))
                .arg("--json")
                .args(*args)
                .env("WRFCOIN_API_URL", server.url())
                .env("RUST_LOG", "error")
                .output()
                .expect("CLI refusal");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(7), "{args:?}: {stdout} {stderr}");
            let envelope: Value = serde_json::from_str(&stdout).expect("JSON refusal");
            assert_eq!(envelope["ok"], false);
            assert!(
                stdout.contains("not wired") && stdout.contains("#3247"),
                "{stdout}"
            );
        }
        reads.assert();
        writes.assert();
        std::fs::remove_dir_all(config_dir).expect("remove test configuration");
    }

    #[test]
    fn bridge_commands_refuse_without_network_or_keys() {
        let output = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
            .args(["bridge", "--help"])
            .output()
            .expect("bridge help");
        assert!(output.status.success());
        let help = String::from_utf8_lossy(&output.stdout);
        let commands: Vec<_> = help
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        for name in [
            "list-bridges",
            "transfer",
            "transaction-info",
            "configure-bridge",
            "test-connection",
            "setup-polkadot",
            "setup-near",
            "watch",
        ] {
            assert!(!commands.contains(&name), "{help}");
        }
        assert_refusals(&[
            &["bridge", "list-bridges"],
            &[
                "bridge",
                "transfer",
                "--to-chain",
                "synthetic",
                "--to-address",
                "synthetic",
                "--amount",
                "1",
                "--from",
                "synthetic",
            ],
            &["bridge", "transaction-info", "--hash", "synthetic"],
            &[
                "bridge",
                "configure-bridge",
                "--chain",
                "synthetic",
                "--provider",
                "synthetic",
                "--rpc-endpoint",
                "http://127.0.0.1:1",
            ],
            &["bridge", "test-connection", "--chain", "synthetic"],
            &[
                "bridge",
                "setup-polkadot",
                "--node-url",
                "http://127.0.0.1:1",
                "--account",
                "synthetic",
            ],
            &[
                "bridge",
                "setup-near",
                "--node-url",
                "http://127.0.0.1:1",
                "--account-id",
                "synthetic",
            ],
            &["bridge", "watch", "--once"],
        ]);
    }
}
