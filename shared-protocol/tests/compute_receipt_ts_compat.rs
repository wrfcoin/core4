use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use wrfcoin_shared_protocol::blockchain::transaction::ComputeReceipt;

#[derive(Debug, Deserialize)]
struct FixtureFile {
    description: String,
    fixture_version: u32,
    receipt: ComputeReceipt,
    signing_message: String,
    inputs: FixtureInputs,
}

#[derive(Debug, Deserialize)]
struct FixtureInputs {
    worker_public_key: String,
    work_factor: u64,
    completed_at: i64,
    nonce: String,
    task_id: String,
    work_hash: String,
    #[allow(dead_code)]
    worker_secret_key: String,
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("shared-protocol has a parent dir")
        .join("sdks/extensions/javascript/compute-receipt/test-vectors")
}

fn load_fixtures(dir: &Path) -> Vec<(String, FixtureFile)> {
    let mut out = Vec::new();
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let fixture: FixtureFile = serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        out.push((path.display().to_string(), fixture));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn fixture_dir_exists_and_has_at_least_three_vectors() {
    let dir = fixture_dir();
    let fixtures = load_fixtures(&dir);
    assert!(
        fixtures.len() >= 3,
        "expected at least 3 fixture vectors in {}, found {}",
        dir.display(),
        fixtures.len(),
    );
}

#[test]
fn ts_generated_receipts_pass_rust_signature_validation() {
    let dir = fixture_dir();
    let fixtures = load_fixtures(&dir);
    assert!(!fixtures.is_empty(), "no fixtures loaded from {}", dir.display());

    for (path, fixture) in &fixtures {
        assert_eq!(
            fixture.fixture_version, 1,
            "{}: unsupported fixture_version",
            path,
        );

        let receipt = &fixture.receipt;
        assert_eq!(receipt.worker_id, fixture.inputs.worker_public_key, "{path}: worker_id mismatch vs declared inputs");
        assert_eq!(receipt.work_factor, fixture.inputs.work_factor, "{path}: work_factor");
        assert_eq!(receipt.completed_at, fixture.inputs.completed_at, "{path}: completed_at");
        assert_eq!(receipt.nonce, fixture.inputs.nonce, "{path}: nonce");
        assert_eq!(receipt.task_id, fixture.inputs.task_id, "{path}: task_id");
        assert_eq!(receipt.work_hash, fixture.inputs.work_hash, "{path}: work_hash");

        assert!(
            receipt.has_valid_signature(),
            "{path} ({}): Rust ComputeReceipt::has_valid_signature() rejected TS-built receipt",
            fixture.description,
        );

        assert_eq!(
            receipt.signing_message(),
            fixture.signing_message,
            "{path}: Rust-built signing message diverges from TS-recorded one",
        );
    }
}

#[test]
fn tampered_fixture_signatures_are_rejected() {
    let dir = fixture_dir();
    let fixtures = load_fixtures(&dir);
    let (path, fixture) = fixtures.first().expect("at least one fixture");

    let mut tampered = fixture.receipt.clone();
    tampered.work_factor = tampered.work_factor.wrapping_add(1);
    assert!(
        !tampered.has_valid_signature(),
        "{path}: bumping work_factor by 1 should invalidate the signature",
    );
}
