use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, station_ownership_message_with_domain},
    crypto::signature::{PrivateKey, PublicKey},
};

const CHAIN: &str = "wrfcoin-test-3383";
const NETWORK: &str = "testnet";

struct Fixture {
    dir: PathBuf,
    owner: PrivateKey,
    governance: PrivateKey,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "station-3383-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&dir).unwrap();
        // Synthetic test keys only; never use an operator key in this suite.
        let owner = PrivateKey::from_bytes(&[0x38; 32]).unwrap();
        let governance = PrivateKey::from_bytes(&[0x83; 32]).unwrap();
        fs::write(dir.join("owner.key"), owner.to_hex()).unwrap();
        fs::write(dir.join("governance.key"), governance.to_hex()).unwrap();
        // Only this module uses the station governance verifier in this test
        // process. Initialize its synthetic allowlist once, avoiding concurrent
        // environment writers; subprocesses receive their own explicit value.
        static GOVERNANCE: std::sync::Once = std::sync::Once::new();
        GOVERNANCE.call_once(|| {
            std::env::set_var(
                "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS",
                PublicKey::from_private_key(&governance).to_hex(),
            );
        });
        Self {
            dir,
            owner,
            governance,
        }
    }

    fn row(&self, id: &str) -> Value {
        let pk = PublicKey::from_private_key(&self.owner);
        let address = pk.to_address();
        let message = station_ownership_message_with_domain(CHAIN, NETWORK, id, &address);
        json!({
            "station_id": id, "owner_address": address,
            "owner_pubkey": pk.to_hex(), "owner_signature": self.owner.sign(&message).to_hex(),
            "latitude": 46.5, "longitude": -92.5, "elevation_meters": 242.5,
            "hardware_model": "phone", "data_quality_tier": "gold",
            "provenance_class": "real", "submitted_at": 1_790_000_000,
        })
    }

    fn run(&self, args: &[String]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
            .args([
                "--config",
                self.dir.join("config.json").to_str().unwrap(),
                "station",
                "build-registration",
            ])
            .args(args)
            .args([
                "--governance-key",
                self.dir.join("governance.key").to_str().unwrap(),
                "--chain-id",
                CHAIN,
                "--network-id",
                NETWORK,
                "--out",
                self.dir.join("out.json").to_str().unwrap(),
            ])
            .env(
                "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS",
                PublicKey::from_private_key(&self.governance).to_hex(),
            )
            .env_remove("WRFCOIN_CHAIN_ID")
            .env_remove("WRFCOIN_NETWORK_ID")
            .output()
            .unwrap()
    }

    fn single_args(&self, row: &Value, private: bool) -> Vec<String> {
        let mut args = Vec::new();
        for field in [
            "station_id",
            "owner_address",
            "latitude",
            "longitude",
            "elevation_meters",
            "hardware_model",
            "data_quality_tier",
            "provenance_class",
            "submitted_at",
        ] {
            args.push(format!("--{}", field.replace('_', "-")));
            args.push(
                row[field]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| row[field].to_string()),
            );
        }
        if private {
            args.extend([
                "--owner-key".into(),
                self.dir.join("owner.key").display().to_string(),
            ]);
        } else {
            for field in ["owner_pubkey", "owner_signature"] {
                args.extend([
                    format!("--{}", field.replace('_', "-")),
                    row[field].as_str().unwrap().into(),
                ]);
            }
        }
        args
    }

    fn transactions(&self) -> Vec<Transaction> {
        let artifact: Value =
            serde_json::from_str(&fs::read_to_string(self.dir.join("out.json")).unwrap()).unwrap();
        assert_eq!(artifact["version"], "station_registration_artifact_v1");
        serde_json::from_value(artifact["transactions"].clone()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn premade_proof_builds_same_verified_transaction_as_owner_key() {
    let f = Fixture::new();
    let row = f.row("mob-3383-control");
    succeeded(&f.run(&f.single_args(&row, true)));
    let legacy = fs::read(f.dir.join("out.json")).unwrap();
    succeeded(&f.run(&f.single_args(&row, false)));
    assert_eq!(legacy, fs::read(f.dir.join("out.json")).unwrap());
    let tx = &f.transactions()[0];
    assert!(tx.verify_station_registration_authorization_with_domain(CHAIN, NETWORK));
    assert!(
        tx.parse_station_registration()
            .unwrap()
            .owner_address_is_bound_to_owner_pubkey()
    );
    assert_eq!(tx.id, Some(tx.calculate_hash()));
}

#[test]
fn invalid_owner_proofs_are_refused_before_governance_or_output() {
    let f = Fixture::new();
    let good = f.row("mob-3383-invalid");
    let wrong = PrivateKey::from_bytes(&[0x99; 32]).unwrap();
    for case in [
        "signature",
        "pubkey",
        "chain",
        "network",
        "station",
        "address",
        "malformed-key",
        "malformed-signature",
    ] {
        let mut row = good.clone();
        match case {
            "signature" => row["owner_signature"] = json!("00".repeat(64)),
            "pubkey" => row["owner_pubkey"] = json!(PublicKey::from_private_key(&wrong).to_hex()),
            "chain" | "network" => {
                let msg = station_ownership_message_with_domain(
                    if case == "chain" {
                        "wrong-chain"
                    } else {
                        CHAIN
                    },
                    if case == "network" {
                        "wrong-network"
                    } else {
                        NETWORK
                    },
                    row["station_id"].as_str().unwrap(),
                    row["owner_address"].as_str().unwrap(),
                );
                row["owner_signature"] = json!(f.owner.sign(&msg).to_hex());
            },
            "station" => row["station_id"] = json!("mob-tampered"),
            "address" => {
                row["owner_address"] = json!(PublicKey::from_private_key(&wrong).to_address());
                let msg = station_ownership_message_with_domain(
                    CHAIN,
                    NETWORK,
                    row["station_id"].as_str().unwrap(),
                    row["owner_address"].as_str().unwrap(),
                );
                row["owner_signature"] = json!(f.owner.sign(&msg).to_hex());
            },
            "malformed-key" => row["owner_pubkey"] = json!("zz"),
            "malformed-signature" => row["owner_signature"] = json!("12"),
            _ => unreachable!(),
        }
        // Missing governance key proves rejection happens in owner preflight.
        let key = f.dir.join("governance.key");
        fs::rename(&key, f.dir.join("saved.key")).unwrap();
        fs::write(f.dir.join("out.json"), "sentinel").unwrap();
        let output = f.run(&f.single_args(&row, false));
        fs::rename(f.dir.join("saved.key"), &key).unwrap();
        assert!(!output.status.success(), "accepted {case}");
        let err = String::from_utf8_lossy(&output.stderr);
        let expected = match case {
            "malformed-key" => "owner_pubkey must be a valid ed25519 public key in hex",
            "malformed-signature" => "owner_signature must be a valid ed25519 signature in hex",
            "address" => "owner_address field is not derived from the owner public key",
            _ => "owner signature does not verify for this chain/network/station/address",
        };
        assert!(err.contains(expected), "{case}: {err}");
        assert!(!err.contains("cannot read key file"), "{case}: {err}");
        assert_eq!(
            fs::read_to_string(f.dir.join("out.json")).unwrap(),
            "sentinel"
        );
    }
}

#[test]
fn owner_input_modes_are_exclusive_and_complete() {
    let f = Fixture::new();
    let row = f.row("mob-3383-flags");
    let mut args = f.single_args(&row, false);
    args.extend([
        "--owner-key".into(),
        f.dir.join("owner.key").display().to_string(),
    ]);
    let output = f.run(&args);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
    let args = f.single_args(&row, false);
    let output = f.run(&args[..args.len() - 2]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--owner-signature <OWNER_SIGNATURE>")
    );
    let mut args = f.single_args(&row, false);
    args.drain(args.len() - 4..args.len() - 2);
    let output = f.run(&args);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--owner-pubkey <OWNER_PUBKEY>"));
    let args = f.single_args(&row, false);
    let output = f.run(&args[..args.len() - 4]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--owner-key <OWNER_KEY>"));
    assert!(!f.dir.join("out.json").exists());
}

#[test]
fn batch_verifies_every_row_and_writes_one_artifact() {
    let f = Fixture::new();
    let rows = json!([f.row("mob-3383-one"), f.row("mob-3383-two")]);
    let batch = f.dir.join("batch.json");
    fs::write(&batch, rows.to_string()).unwrap();
    succeeded(&f.run(&["--batch".into(), batch.display().to_string()]));
    let txs = f.transactions();
    assert_eq!(txs.len(), 2);
    for tx in txs {
        assert!(tx.verify_station_registration_authorization_with_domain(CHAIN, NETWORK));
        assert!(
            tx.parse_station_registration()
                .unwrap()
                .owner_address_is_bound_to_owner_pubkey()
        );
    }
    assert!(
        !f.run(&[
            "--batch".into(),
            batch.display().to_string(),
            "--owner-key".into(),
            f.dir.join("owner.key").display().to_string()
        ])
        .status
        .success()
    );
}

#[test]
fn batch_lists_all_failures_and_preserves_existing_output() {
    let f = Fixture::new();
    let mut rows = json!([
        f.row("mob-good"),
        f.row("mob-bad-proof"),
        f.row("mob-bad-address"),
        f.row("mob-bad-schema")
    ]);
    rows[1]["owner_signature"] = json!("00".repeat(64));
    rows[2]["owner_address"] = json!("not-an-address");
    rows[3]["latitude"] = json!("bad-number");
    let batch = f.dir.join("batch.json");
    fs::write(&batch, rows.to_string()).unwrap();
    fs::write(f.dir.join("out.json"), "sentinel").unwrap();
    fs::remove_file(f.dir.join("governance.key")).unwrap();
    let output = f.run(&["--batch".into(), batch.display().to_string()]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    for row in ["row 2", "row 3", "row 4"] {
        assert!(err.contains(row), "{err}");
    }
    assert_eq!(
        fs::read_to_string(f.dir.join("out.json")).unwrap(),
        "sentinel"
    );
}

#[test]
fn empty_or_malformed_batch_is_refused_without_creating_output() {
    let f = Fixture::new();
    let batch = f.dir.join("batch.json");
    for raw in ["[]", "{}", "["] {
        fs::write(&batch, raw).unwrap();
        let output = f.run(&["--batch".into(), batch.display().to_string()]);
        assert!(!output.status.success());
        let expected = if raw == "[]" {
            "--batch must contain at least one row"
        } else {
            "--batch must be a JSON array"
        };
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
        assert!(!f.dir.join("out.json").exists());
    }
}

#[test]
fn oversized_station_ids_are_refused_before_governance_signing() {
    let f = Fixture::new();
    let row = f.row(&"x".repeat(129));
    fs::remove_file(f.dir.join("governance.key")).unwrap();
    fs::write(f.dir.join("out.json"), "sentinel").unwrap();
    for private in [false, true] {
        let output = f.run(&f.single_args(&row, private));
        assert!(!output.status.success());
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains("station_id exceeds max length"), "{err}");
        assert_eq!(
            fs::read_to_string(f.dir.join("out.json")).unwrap(),
            "sentinel"
        );
    }
    let batch = f.dir.join("batch.json");
    fs::write(&batch, json!([row, f.row(&"é".repeat(65))]).to_string()).unwrap();
    let output = f.run(&["--batch".into(), batch.display().to_string()]);
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("row 1") && err.contains("row 2"), "{err}");
    assert!(err.contains("station_id exceeds max length"), "{err}");
    assert_eq!(
        fs::read_to_string(f.dir.join("out.json")).unwrap(),
        "sentinel"
    );
}

#[test]
fn artifact_over_api_body_ceiling_is_refused_without_writing() {
    let f = Fixture::new();
    let rows: Vec<_> = (0..64).map(|i| f.row(&format!("mob-cap-{i:03}"))).collect();
    let batch = f.dir.join("batch.json");
    fs::write(&batch, serde_json::to_string(&rows).unwrap()).unwrap();
    fs::write(f.dir.join("out.json"), "sentinel").unwrap();
    let output = f.run(&["--batch".into(), batch.display().to_string()]);
    assert!(
        !output.status.success(),
        "accepted a body larger than the launch API's 64 KiB ceiling"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("65536"));
    assert_eq!(
        fs::read_to_string(f.dir.join("out.json")).unwrap(),
        "sentinel"
    );
}

// Calibrate the exact mobile export shape through the real executable. Every
// private key is generated here and is used only in this disposable fixture.
#[test]
fn realistic_phone_batch_artifact_size_calibration() {
    use rand::{SeedableRng, rngs::StdRng};
    use wrfcoin_cli::constants::{
        CALIBRATED_MOBILE_STATION_BATCH_ROWS, MAX_STATION_ARTIFACT_BYTES,
    };
    use wrfcoin_shared_protocol::crypto::hash::sha256_hash;

    let mut rng = StdRng::seed_from_u64(0xBA7C_A11B);
    let mut f = Fixture::new();
    f.governance = PrivateKey::generate(&mut rng);
    fs::write(f.dir.join("governance.key"), f.governance.to_hex()).unwrap();
    let rows: Vec<_> = (0..64)
        .map(|_| {
            let owner = PrivateKey::generate(&mut rng);
            let pk = PublicKey::from_private_key(&owner);
            let address = format!("0x{}", pk.to_address().trim_start_matches("wrf"));
            // Mirrors backend shared/mobile-station-messages.ts: domain,
            // separator, 20 address bytes, reserved slot byte, SHA-256[:16].
            let mut id_preimage = b"wrfcoin_mobile_station_v1\0".to_vec();
            id_preimage.extend(hex::decode(&address[2..]).unwrap());
            id_preimage.push(0);
            let id = format!("mob-{}", &sha256_hash(&id_preimage).to_hex()[..32]);
            let proof = station_ownership_message_with_domain("777", NETWORK, &id, &address);
            json!({
                "station_id": id, "owner_address": address,
                "owner_pubkey": pk.to_hex(), "owner_signature": owner.sign(&proof).to_hex(),
                "latitude": -89.9, "longitude": -179.9,
                "hardware_model": "phone-barometer", "data_quality_tier": "unverified",
                "provenance_class": "real", "submitted_at": 1_790_000_000,
            })
        })
        .collect();
    let batch = f.dir.join("batch.json");
    // Seeded fixture keys pin the decimal byte-array hashes as well as signature
    // widths. A different seed can move byte counts, so the recommendation has
    // headroom and never replaces the actual CLI byte ceiling.
    let cases = [
        (1, 3091),
        (21, 62631),
        (22, 65609),
        (30, 89384),
        (63, 187556),
    ];
    let mut largest_accepted = 0;
    for (n, expected_bytes) in cases {
        fs::write(&batch, serde_json::to_string(&rows[..n]).unwrap()).unwrap();
        fs::write(f.dir.join("out.json"), "previous-complete-artifact").unwrap();
        // Domain flags here override the fixture helper's normal test domain.
        let output = Command::new(env!("CARGO_BIN_EXE_wrfcoin"))
            .args([
                "--config",
                f.dir.join("config.json").to_str().unwrap(),
                "station",
                "build-registration",
                "--batch",
                batch.to_str().unwrap(),
                "--governance-key",
                f.dir.join("governance.key").to_str().unwrap(),
                "--chain-id",
                "777",
                "--network-id",
                NETWORK,
                "--out",
                f.dir.join("out.json").to_str().unwrap(),
            ])
            .env(
                "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS",
                PublicKey::from_private_key(&f.governance).to_hex(),
            )
            .env_remove("WRFCOIN_CHAIN_ID")
            .env_remove("WRFCOIN_NETWORK_ID")
            .output()
            .unwrap();
        if output.status.success() {
            assert!(
                expected_bytes <= MAX_STATION_ARTIFACT_BYTES,
                "oversize artifact accepted"
            );
            let bytes = fs::read(f.dir.join("out.json")).unwrap();
            assert_eq!(
                bytes.len(),
                expected_bytes,
                "pretty artifact size for {n} rows"
            );
            let artifact: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                bytes,
                (serde_json::to_string_pretty(&artifact).unwrap() + "\n").as_bytes()
            );
            assert_eq!(artifact["transactions"].as_array().unwrap().len(), n);
            for value in artifact["transactions"].as_array().unwrap() {
                let tx: Transaction = serde_json::from_value(value.clone()).unwrap();
                let payload = tx.parse_station_registration().unwrap();
                assert_eq!(payload.owner_pubkey.as_ref().unwrap().len(), 64);
                assert_eq!(payload.auth_pubkey.as_ref().unwrap().len(), 64);
                assert_eq!(payload.owner_signature.as_ref().unwrap().len(), 128);
                assert_eq!(payload.auth_signature.as_ref().unwrap().len(), 128);
            }
            largest_accepted = n;
            println!("CALIBRATION rows={n} pretty_bytes={}", bytes.len());
        } else {
            assert!(
                expected_bytes > MAX_STATION_ARTIFACT_BYTES,
                "fitting artifact refused"
            );
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            assert!(
                diagnostic.contains("station artifact is"),
                "unexpected refusal"
            );
            let size = diagnostic
                .split("station artifact is ")
                .nth(1)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .parse::<usize>()
                .unwrap();
            assert_eq!(size, expected_bytes, "refused artifact size for {n} rows");
            assert_eq!(
                fs::read_to_string(f.dir.join("out.json")).unwrap(),
                "previous-complete-artifact"
            );
            println!("CALIBRATION rows={n} pretty_bytes={size} refused=true");
        }
    }
    assert_eq!(largest_accepted, CALIBRATED_MOBILE_STATION_BATCH_ROWS);
}

#[test]
fn batch_address_refusal_names_the_row_field() {
    let f = Fixture::new();
    let mut row = f.row("mob-address-diagnostic");
    let wrong = PrivateKey::from_bytes(&[0x99; 32]).unwrap();
    row["owner_address"] = json!(PublicKey::from_private_key(&wrong).to_address());
    let message = station_ownership_message_with_domain(
        CHAIN,
        NETWORK,
        row["station_id"].as_str().unwrap(),
        row["owner_address"].as_str().unwrap(),
    );
    row["owner_signature"] = json!(f.owner.sign(&message).to_hex());
    let batch = f.dir.join("batch.json");
    fs::write(
        &batch,
        json!([f.row("mob-good-diagnostic"), row]).to_string(),
    )
    .unwrap();
    let output = f.run(&["--batch".into(), batch.display().to_string()]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("row 2: owner_address field"), "{err}");
    assert!(!err.contains("--owner-address"), "{err}");
    assert!(!f.dir.join("out.json").exists());
}
