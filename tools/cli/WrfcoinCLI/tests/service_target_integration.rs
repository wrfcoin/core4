//! #3204: service-family clients must surface backend's honest-gap 501
//! (backend#1118) and a wrong-route 404 loudly, naming the URL — never a
//! silent failure or a fake success.

use mockito::Server;
use std::time::Duration;
use wrfcoin_cli::{ApiClient, ApiConfig, CliErrorCode, ServiceTarget};

fn config_for(server: &Server) -> ApiConfig {
    ApiConfig {
        base_url: format!("{}/api", server.url()),
        timeout: Duration::from_secs(1),
        retry_attempts: 0,
        retry_delay: Duration::from_millis(1),
        ..ApiConfig::for_service(ServiceTarget::Compute)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn honest_gap_501_is_surfaced_with_reason_and_pointer() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let _submit = server
        .mock("POST", "/api/compute/submit")
        .with_status(501)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"success":false,"unavailable":true,
                "reason":"Generic job submission to Flux not yet wired; the job was NOT accepted.",
                "error":{"code":"NOT_IMPLEMENTED","capability":"compute.submit",
                         "message":"Generic job submission to Flux not yet wired; the job was NOT accepted.",
                         "see":"wrfcoin/backend#1118"},
                "timestamp":"2026-09-16T00:00:00.000Z"}"#,
        )
        .expect(1)
        .create_async()
        .await;

    let client = ApiClient::new(config_for(&server))?;
    let err = client
        .post::<serde_json::Value>("/compute/submit", Some(serde_json::json!({"task": "x", "network": "flux"})))
        .await
        .expect_err("501 must be an error, not a fake success");

    let cli = err
        .downcast_ref::<wrfcoin_cli::errors::CliError>()
        .expect("CliError");
    assert_eq!(cli.code(), CliErrorCode::NotFound);
    let msg = cli.to_string();
    assert!(msg.contains("not wired"), "message: {msg}");
    assert!(msg.contains("job was NOT accepted"), "message: {msg}");
    assert!(msg.contains("[see wrfcoin/backend#1118]"), "message: {msg}");
    assert!(msg.contains("/api/compute/submit"), "message: {msg}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_route_404_names_the_url_and_service() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let _missing = server
        .mock("GET", "/api/iot/readings")
        .with_status(404)
        .with_body(r#"{"success":false,"error":"Endpoint not found"}"#)
        .expect(1)
        .create_async()
        .await;

    let client = ApiClient::new(config_for(&server))?;
    let err = client
        .get::<serde_json::Value>("/iot/readings", None)
        .await
        .expect_err("404 must be an error");
    let cli = err
        .downcast_ref::<wrfcoin_cli::errors::CliError>()
        .expect("CliError");
    assert_eq!(cli.code(), CliErrorCode::NotFound);
    let msg = cli.to_string();
    assert!(msg.contains("no such route on this service"), "message: {msg}");
    assert!(msg.contains(&format!("{}/api/iot/readings", server.url())), "message: {msg}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn plain_501_without_envelope_is_reported_verbatim() -> anyhow::Result<()> {
    let mut server = Server::new_async().await;
    let _plain = server
        .mock("GET", "/api/storage/networks")
        .with_status(501)
        .with_body("Not Implemented")
        .expect(1)
        .create_async()
        .await;
    let client = ApiClient::new(config_for(&server))?;
    let err = client
        .get::<serde_json::Value>("/storage/networks", None)
        .await
        .expect_err("501 must be an error");
    let msg = err.to_string();
    assert!(msg.contains("501") && msg.contains("Not Implemented"), "message: {msg}");
    Ok(())
}

mod route_gaps_3247 {
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
    fn batch_weather_refuses_before_file_or_stdin_requests() {
        use std::io::Write;
        use std::process::Stdio;
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("batch-gap-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let batch = directory.join("synthetic.jsonl");
        let input = concat!(
            "{\"command\":\"weather query-history\",\"args\":{\"start_date\":\"2026-01-01\",\"end_date\":\"2026-01-02\"}}\n",
            "{\"command\":\"weather submit\",\"args\":{\"parameter\":\"temperature\",\"value\":22,\"latitude\":1,\"longitude\":2,\"station_id\":\"synthetic\"}}\n"
        );
        std::fs::write(&batch, input).unwrap();
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
        for (file, fail_fast) in [
            (Some(batch.clone()), false),
            (Some(batch.clone()), true),
            (None, false),
            (Some(directory.join("missing.jsonl")), false),
        ] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_wrfcoin"));
            command
                .arg("--config")
                .arg(directory.join("config.json"))
                .arg("--json")
                .args(["batch", "run"])
                .env("WRFCOIN_API_URL", server.url())
                .env("RUST_LOG", "error")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if let Some(path) = file {
                command.arg("--file").arg(path);
            }
            if fail_fast {
                command.arg("--fail-fast");
            }
            let mut child = command.spawn().unwrap();
            if let Some(mut stdin) = child.stdin.take() {
                // An immediate refusal may close stdin before the synthetic input
                // is written. Both behaviors must produce the same refusal.
                if let Err(error) = stdin.write_all(input.as_bytes()) {
                    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
                }
            }
            let output = child.wait_with_output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(7), "{stdout} {stderr}");
            let envelope: Value = serde_json::from_str(&stdout).unwrap();
            assert_eq!(envelope["ok"], false);
            assert!(
                stdout.contains("not wired") && stdout.contains("#3247"),
                "{stdout}"
            );
        }
        let help = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
            .args(["batch", "--help"])
            .output()
            .unwrap();
        assert!(help.status.success());
        assert!(
            !String::from_utf8_lossy(&help.stdout)
                .lines()
                .any(|line| line.trim_start().starts_with("run "))
        );
        reads.assert();
        writes.assert();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unsupported_weather_commands_refuse_before_files_keys_or_network() {
        // History comes first so the main RED control exercises only HTTP,
        // without trying to load a wallet signer or an offline queue.
        assert_refusals(&[
            &[
                "weather",
                "query-history",
                "--start-date",
                "2026-01-01",
                "--end-date",
                "2026-01-02",
            ],
            &[
                "weather",
                "submit",
                "--parameter",
                "temperature",
                "--value",
                "20",
                "--latitude",
                "0",
                "--longitude",
                "0",
                "--unit",
                "celsius",
                "--station-id",
                "synthetic",
            ],
            &[
                "weather",
                "submit-batch",
                "--file",
                "nonexistent-synthetic-input.json",
            ],
            &[
                "weather",
                "submit-wind",
                "--speed",
                "2",
                "--direction",
                "180",
                "--latitude",
                "0",
                "--longitude",
                "0",
                "--unit",
                "m/s",
            ],
            &[
                "weather",
                "create-nft",
                "--title",
                "synthetic",
                "--data-points",
                "synthetic",
            ],
            &["weather", "sync"],
        ]);
    }

    #[test]
    fn unsupported_nft_commands_refuse_without_network() {
        assert_refusals(&[
            &["nft", "categories"],
            &["nft", "list", "--nft-type", "weather"],
            &[
                "nft",
                "create",
                "--title",
                "synthetic",
                "--description",
                "synthetic",
                "--computation-type",
                "Weather",
                "--reward",
                "0",
                "--creator",
                "synthetic",
            ],
            &["nft", "submit", "--id", "synthetic", "--result", "{}"],
            &["nft", "add-category", "synthetic"],
            &["nft", "register-verifier", "--supported-types", "Weather"],
            &["nft", "list-verifiers"],
            &[
                "nft",
                "verify",
                "--id",
                "synthetic",
                "--result-hash",
                "synthetic",
                "--decision",
                "approve",
            ],
        ]);
    }

    #[test]
    fn unsupported_governance_history_refuses_without_network() {
        assert_refusals(&[&["governance", "voting-history"]]);
    }

    #[test]
    fn nft_list_uses_the_registered_node_route_and_owner_filter() {
        let config_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("nft-list-test-{}", uuid::Uuid::new_v4()));
        let mut server = Server::new();
        let owner = format!("0x{}", "01".repeat(32));
        let listing = serde_json::json!({"items": [], "total": 0, "limit": 25, "offset": 0});
        let route = server
            .mock("GET", "/nft/list")
            .match_query(Matcher::UrlEncoded("owner".into(), owner.clone()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(listing.to_string())
            .expect(1)
            .create();
        let output = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
            .arg("--config")
            .arg(config_dir.join("config.json"))
            .args(["--json", "nft", "list", "--owner", &owner])
            .env("WRFCOIN_API_URL", server.url())
            .env("RUST_LOG", "error")
            .output()
            .expect("NFT list");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).expect("JSON output");
        assert_eq!(result["data"]["listing"], listing);
        route.assert();
        std::fs::remove_dir_all(config_dir).expect("remove test configuration");
    }

    #[test]
    fn unsupported_service_commands_are_hidden_from_help() {
        for (group, unavailable, retained) in [
            (
                "weather",
                vec![
                    "submit",
                    "submit-batch",
                    "submit-wind",
                    "create-nft",
                    "query-history",
                    "sync",
                ],
                "queue",
            ),
            (
                "nft",
                vec![
                    "create",
                    "submit",
                    "categories",
                    "add-category",
                    "register-verifier",
                    "list-verifiers",
                    "verify",
                ],
                "show",
            ),
            ("governance", vec!["voting-history"], "list-proposals"),
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
                .args([group, "--help"])
                .output()
                .expect("CLI help");
            assert!(output.status.success());
            let help = String::from_utf8_lossy(&output.stdout);
            let commands: Vec<_> = help
                .lines()
                .filter_map(|line| line.split_whitespace().next())
                .collect();
            assert!(commands.contains(&retained), "{help}");
            for command in unavailable {
                assert!(!commands.contains(&command), "{group} {command}: {help}");
            }
        }
    }
}
