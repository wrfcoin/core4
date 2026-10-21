#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forecast-proto"));
    command.env_remove("WRFCOIN_FORECAST_ORACLE_KEY")
        .env_remove("WRFCOIN_FORECAST_FORECASTER_KEY");
    command
}

fn run_command() -> Command {
    let mut command = command();
    command.args(["run", "--station-id", "test", "--token", "test", "--oracle-token", "test",
        "--evidence-dir", "/unused", "--observations-dir", "/nonexistent-forecast-test-input"])
        .env("WRFCOIN_CHAIN_ID", "777").env("WRFCOIN_NETWORK_ID", "testnet");
    command
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn generated_keys_are_private_and_never_printed() {
    let dir = tempfile::tempdir().unwrap();
    let output = command().args(["keygen", "--out"]).arg(dir.path()).output().unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let metadata = std::fs::read_to_string(dir.path().join("keys.json")).unwrap();
    for role in ["oracle", "governance", "station_owner", "forecaster", "producer"] {
        let path = dir.path().join(format!("{role}.key"));
        let key = std::fs::read_to_string(&path).unwrap();
        assert!(!key.is_empty());
        assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(!text(&output).contains(&key));
        assert!(!metadata.contains(&key));
    }
    assert!(!metadata.contains("private_key"));
    let second = command().args(["keygen", "--out"]).arg(dir.path()).output().unwrap();
    assert!(!second.status.success(), "must refuse overwriting an existing private key");
}

#[test]
fn file_permissions_are_checked_before_use_and_errors_hide_key_material() {
    let dir = tempfile::tempdir().unwrap();
    let key = "19".repeat(32);
    let path = dir.path().join("oracle.key");
    std::fs::write(&path, &key).unwrap();
    for mode in [0o644, 0o640, 0o601, 0o700, 0o1600] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        let output = run_command().arg("--oracle-key").arg(&path)
            .env("WRFCOIN_FORECAST_FORECASTER_KEY", &key).output().unwrap();
        assert!(!output.status.success());
        assert!(text(&output).contains("permissions must be at most 0600"));
        assert!(!text(&output).contains(&key));
    }
    for mode in [0o600, 0o400] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        let output = run_command().arg("--oracle-key").arg(&path)
            .env("WRFCOIN_FORECAST_FORECASTER_KEY", &key).output().unwrap();
        assert!(!output.status.success());
        assert!(text(&output).contains("reading observation artifacts"), "key must load before missing input error: {}", text(&output));
        assert!(!text(&output).contains(&key));
    }
}

#[test]
fn environment_keys_and_help_do_not_disclose_secrets() {
    let key = "27".repeat(32);
    let output = run_command().env("WRFCOIN_FORECAST_ORACLE_KEY", &key)
        .env("WRFCOIN_FORECAST_FORECASTER_KEY", &key).output().unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("reading observation artifacts"));
    assert!(!text(&output).contains(&key));
    let help = command().args(["run", "--help"]).env("WRFCOIN_FORECAST_ORACLE_KEY", &key).output().unwrap();
    assert!(help.status.success());
    assert!(text(&help).contains("WRFCOIN_FORECAST_ORACLE_KEY"));
    assert!(!text(&help).contains(&key));
    let malformed = "PRIVATE_SENTINEL_MALFORMED";
    let output = run_command().env("WRFCOIN_FORECAST_ORACLE_KEY", malformed).output().unwrap();
    assert!(!output.status.success());
    assert!(text(&output).contains("invalid private key encoding"));
    assert!(!text(&output).contains(malformed));
}
