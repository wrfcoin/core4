//! Actual binary + signed receipt boundary, with synthetic loopback transport.
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::{Duration, Instant};
use wrfcoin_forecast_score::{ChainBinding, ForecastArtifactManifest, ForecastVerificationReceipt};

fn execute(
    features: Option<Value>,
    window: &[f64],
    truth: f64,
    expected: Option<f64>,
) -> Vec<Vec<u8>> {
    let persistence = features
        .as_ref()
        .and_then(Value::as_array)
        .is_some_and(|list| list.contains(&json!("forecast_skill_baseline")));
    let directory = tempfile::tempdir().unwrap();
    let observations = directory.path().join("observations");
    let evidence = directory.path().join("evidence");
    std::fs::create_dir(&observations).unwrap();
    for (index, value) in window.iter().chain([&truth]).enumerate() {
        let timestamp = 1_700_000_000
            + if index == window.len() {
                (index as i64 - 1) * 3600 + 21600
            } else {
                index as i64 * 3600
            };
        let doc = json!({"reveal_body": {"station_id":"synthetic-station", "timestamp":timestamp,
            "payload":{"temperature_c":value,"humidity_pct":value,"pressure_hpa":value}}});
        std::fs::write(
            observations.join(format!("obs-{index}.json")),
            doc.to_string(),
        )
        .unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let mut view = json!({"chain_id":"777","network_id":"testnet","height":99});
    if let Some(features) = features {
        view["active_features"] = features;
    }
    let (stop, stopped) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if stopped.try_recv().is_ok() {
                        break;
                    }
                    assert!(Instant::now() < deadline, "missing binary request");
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                },
                Err(error) => panic!("fixture accept: {error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut part = [0; 4096];
                let n = stream.read(&mut part).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&part[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
            let length = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(|s| s.parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while bytes.len() < header_end + length {
                let mut part = [0; 4096];
                let n = stream.read(&mut part).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&part[..n]);
            }
            assert!(
                header
                    .to_ascii_lowercase()
                    .contains("authorization: bearer synthetic-token")
            );
            let target = header.lines().next().unwrap().to_string();
            let payload = if length == 0 {
                Value::Null
            } else {
                serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
            };
            let response = if target.starts_with("GET /transactions/oracle/pending?limit=1 ") {
                view.clone()
            } else {
                assert!(
                    target.starts_with("POST /transactions/"),
                    "unexpected outbound request {target}"
                );
                json!({"ok":true})
            }
            .to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
            requests.push((target, payload));
        }
        requests
    });
    let output = Command::new(env!("CARGO_BIN_EXE_forecast-proto"))
        .args([
            "run",
            "--node-url",
            &format!("http://{address}"),
            "--station-id",
            "synthetic-station",
            "--token",
            "synthetic-token",
            "--oracle-token",
            "synthetic-token",
            "--observations-dir",
        ])
        .arg(&observations)
        .arg("--evidence-dir")
        .arg(&evidence)
        .env("WRFCOIN_CHAIN_ID", "777")
        .env("WRFCOIN_NETWORK_ID", "testnet")
        .env("WRFCOIN_FORECAST_ORACLE_KEY", "19".repeat(32))
        .env("WRFCOIN_FORECAST_FORECASTER_KEY", "27".repeat(32))
        .env_remove("WRFCOIN_FORECAST_MODEL")
        .env_remove("WRFCOIN_FORECAST_ONNX")
        .env_remove("WRFCOIN_SIGNING_PROTOCOL_VERSION")
        .output()
        .unwrap();
    stop.send(()).unwrap();
    let requests = server.join().unwrap();
    if let Some(skill) = expected {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(requests.len(), 4);
        for (request, prefix) in requests.iter().skip(1).zip([
            "POST /transactions/forecast/commit ",
            "POST /transactions/forecast/reveal ",
            "POST /transactions/oracle/attestation ",
        ]) {
            assert!(request.0.starts_with(prefix));
        }
        let receipt: ForecastVerificationReceipt =
            serde_json::from_slice(&std::fs::read(evidence.join("06-receipt.json")).unwrap())
                .unwrap();
        assert_eq!(receipt.metrics.skill_score, skill);
        assert!(receipt.has_valid_signature_with(&ChainBinding::new("777", "testnet").unwrap()));
        let mut manifest: ForecastArtifactManifest =
            serde_json::from_slice(&std::fs::read(evidence.join("03-manifest.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest.model_id.ends_with(";skill=persistence-rmse/v1"),
            persistence
        );
        assert_eq!(manifest.manifest_digest(), receipt.manifest_digest);
        assert_eq!(
            requests[2].1["verification_receipt"]["metrics"]["skill_score"],
            json!(skill)
        );
        if manifest.model_id.ends_with(";skill=persistence-rmse/v1") {
            manifest.model_id = "persistence-trend/v1".to_string();
            assert_ne!(
                manifest.manifest_digest(),
                receipt.manifest_digest,
                "strategy must be digest-bound"
            );
        }
        [
            "03-manifest.json",
            "05-score.json",
            "06-receipt.json",
            "02-forecast.json",
        ]
        .iter()
        .map(|name| std::fs::read(evidence.join(name)).unwrap())
        .collect()
    } else {
        assert_eq!(
            requests.len(),
            1,
            "invalid feature view or score must publish nothing"
        );
        assert!(!output.status.success());
        Vec::new()
    }
}

#[test]
fn binary_persistence_copy_and_zero_error_tie_sign_zero_skill() {
    let active = Some(json!(["forecast_skill_baseline"]));
    execute(active.clone(), &[4.0, -2.0, -2.0, 4.0], 6.0, Some(0.0));
    execute(active, &[4.0, 4.0], 4.0, Some(0.0));
}

#[test]
fn binary_improved_forecast_signs_positive_skill() {
    // Hourly 0,1,2 trend extrapolates +3 to5; truth6, last input2 => skill3/4.
    execute(
        Some(json!(["forecast_skill_baseline"])),
        &[0.0, 1.0, 2.0],
        6.0,
        Some(0.75),
    );
}

#[test]
fn binary_off_and_older_node_keep_manifest_report_receipt_and_forecast_bytes() {
    let older = execute(None, &[4.0, 4.0], 4.0, Some(1.0));
    let off = execute(Some(json!([])), &[4.0, 4.0], 4.0, Some(1.0));
    let unrelated = execute(
        Some(json!(["reward_gross_outlier_guard"])),
        &[4.0, 4.0],
        4.0,
        Some(1.0),
    );
    assert_eq!(older, off);
    assert_eq!(older, unrelated);
}

#[test]
fn binary_malformed_feature_view_never_publishes_a_transaction() {
    for features in [json!("forecast_skill_baseline"), json!([true]), Value::Null] {
        execute(Some(features), &[4.0, 4.0], 4.0, None);
    }
}

#[test]
fn binary_invalid_active_score_never_publishes_even_a_commit() {
    execute(
        Some(json!(["forecast_skill_baseline"])),
        &[1e308, 1e308],
        1e308,
        None,
    );
}

#[test]
fn binary_aggregate_overflow_never_publishes_even_a_commit() {
    // Each error and its square are finite; their three-variable sum overflows.
    execute(
        Some(json!(["forecast_skill_baseline"])),
        &[0.0, 0.0],
        1e154,
        None,
    );
}
