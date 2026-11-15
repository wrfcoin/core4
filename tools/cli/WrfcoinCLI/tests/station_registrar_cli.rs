//! `wrfcoin --json station registrar-sign` through the real binary: the
//! one-line JSON envelope the testnet registrar service parses (the last
//! stdout line; `ok`, then `data` or `error.message`). This build pins the
//! registrar keys for chain 888 only, so a call for any other chain instance is
//! refused; the signing path itself is tested in `commands::station_registrar`
//! with test pins.

use std::process::Command;

use serde_json::Value;

/// Chain 888's public block-1 hash (`packaging/windows/chain-888.conf`).
const CHAIN_888: &str = "553a3758386d9631037d7469686acb4a121ced81dcf5cde256e2d05667c5f332"; // pragma: allowlist secret -- public genesis hash
/// A chain instance with no registrar pin (a made-up block-1 hash).
const OTHER_CHAIN: &str = "1111111111111111111111111111111111111111111111111111111111111111"; // pragma: allowlist secret -- not a real chain

fn run(args: &[&str]) -> (bool, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
        .arg("--json")
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .expect("run wrfcoin");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let last = stdout.trim().lines().last().unwrap_or_default().to_string();
    let envelope: Value = serde_json::from_str(&last)
        .unwrap_or_else(|e| panic!("last stdout line is not JSON ({e}): {stdout}"));
    (out.status.success(), envelope)
}

#[test]
fn an_unpinned_chain_is_refused_with_a_one_line_envelope() {
    let dir = std::env::temp_dir().join(format!("wrfcoin-registrar-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let row = dir.join("row.json");
    std::fs::write(&row, "{}").unwrap();
    let row = row.to_str().unwrap();
    let base = [
        "station",
        "registrar-sign",
        "--row-file",
        row,
        "--chain-id",
        "888",
        "--network-id",
        "testnet",
        "--genesis-hash",
        OTHER_CHAIN,
        "--expected-nonce",
        "0",
    ];

    let (success, envelope) = run(&[&base[..], &["--check"]].concat());
    assert!(!success);
    assert_eq!(envelope["ok"], false, "{envelope}");
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("pins no station registrar key"),
        "{message}"
    );

    // clap refuses --check together with a key file before the command runs.
    let status = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
        .arg("--json")
        .args([&base[..], &["--check", "--key-file", row]].concat())
        .output()
        .expect("run wrfcoin")
        .status;
    assert!(!status.success());

    // Chain 888 is pinned: the same call gets past the pin gate and is refused
    // for the empty row instead.
    let mut on_888 = base.to_vec();
    on_888[9] = CHAIN_888;
    let (success, envelope) = run(&[&on_888[..], &["--check"]].concat());
    assert!(!success);
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(!message.contains("pins no station registrar key"), "{message}");
    assert!(message.contains("not a registrar row"), "{message}");

    let mut nonce_one = base.to_vec();
    nonce_one[11] = "1";
    let (_, envelope) = run(&[&nonce_one[..], &["--check"]].concat());
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("new stations only")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
