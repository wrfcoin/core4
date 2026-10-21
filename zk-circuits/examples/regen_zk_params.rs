//! Regenerate the embedded Groth16 key bundles in `native-chain/zk-params/`.
//!
//! Run from the workspace root:
//!
//! ```bash
//! cargo run --release -p wrfcoin-zk-circuits --example regen_zk_params
//! ```
//!
//! # When to run this
//!
//! **Any time a circuit's constraint system changes.** A Groth16 proving key is
//! bound to the exact R1CS it was generated from; a key built for an older
//! circuit produces proofs that silently fail verification. The public-input
//! count usually stays the same across such a change, so deserialization keeps
//! working and `verify` just returns `false` — there is no error to notice.
//!
//! This exact drift is what broke the `zk_service` suite (#2283/#2274/#2240):
//! the bundles were generated 2026-03-08 and the circuits moved to Poseidon
//! commitments on 2026-03-16. Nothing regenerated them, and nothing checked.
//! `zk_params_match_current_circuits` in `native-chain/tests/zk_params_drift.rs`
//! is the guard; this example is how you make it green again.
//!
//! # Security
//!
//! Single-party trusted setup via `OsRng` — the same trust model the existing
//! bundles were generated under. The toxic waste lives transiently in this
//! process's memory. Acceptable for testnet; mainnet requires an MPC ceremony
//! (see `native-chain/zk-params/PROVENANCE.md` and issues #717, #716).
//!
//! After running, paste the printed SHA-256 table into `PROVENANCE.md`.

use ark_bn254::Bn254;
use ark_groth16::{PreparedVerifyingKey, ProvingKey};
use ark_serialize::CanonicalSerialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use wrfcoin_zk_circuits::circuits::confidential_transfer::ConfidentialTransferCircuit;
use wrfcoin_zk_circuits::circuits::range_proof::RangeProofCircuit;
use wrfcoin_zk_circuits::circuits::selective_disclosure::SelectiveDisclosureCircuit;
use wrfcoin_zk_circuits::circuits::weather_model::WeatherModelCircuit;
use wrfcoin_zk_circuits::proving;

/// Mirror of `zk_service::serialize_key_pair`: proving key then prepared
/// verifying key, both `ark-serialize` compressed, concatenated.
fn serialize_pair(pk: &ProvingKey<Bn254>, pvk: &PreparedVerifyingKey<Bn254>) -> Vec<u8> {
    let mut bytes = Vec::new();
    pk.serialize_compressed(&mut bytes).expect("serialize pk");
    pvk.serialize_compressed(&mut bytes).expect("serialize pvk");
    bytes
}

fn params_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("zk-circuits has a parent directory")
        .join("native-chain/zk-params")
}

macro_rules! regen {
    ($rows:expr, $dir:expr, $file:expr, $desc:expr, $blank:expr) => {{
        println!("generating {} ...", $file);
        let (pk, pvk) = proving::generate_keys($blank).expect("key generation");
        let bytes = serialize_pair(&pk, &pvk);
        let path = $dir.join($file);
        std::fs::write(&path, &bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        println!(
            "  {} bytes | {} public inputs | {} constraints (FFT domain - 1)",
            bytes.len(),
            pk.vk.gamma_abc_g1.len() - 1,
            pk.h_query.len(),
        );
        $rows.push(($file, $desc, hex::encode(Sha256::digest(&bytes))));
    }};
}

fn main() {
    let dir = params_dir();
    assert!(dir.is_dir(), "{} does not exist", dir.display());
    println!("writing to {}\n", dir.display());

    let mut rows: Vec<(&str, &str, String)> = Vec::new();

    regen!(
        rows,
        dir,
        "range-proof-64.bin",
        "64-bit range proof",
        RangeProofCircuit::blank(64)
    );
    regen!(
        rows,
        dir,
        "confidential-transfer-1-1-64.bin",
        "1-input 1-output confidential transfer (64-bit)",
        ConfidentialTransferCircuit::blank(1, 1, 64)
    );
    regen!(
        rows,
        dir,
        "selective-disclosure-3.bin",
        "3-field selective disclosure",
        SelectiveDisclosureCircuit::blank(3)
    );
    regen!(
        rows,
        dir,
        "weather-model-2.bin",
        "2-variable weather model proof",
        WeatherModelCircuit::blank(2)
    );

    println!("\n--- paste into native-chain/zk-params/PROVENANCE.md ---\n");
    println!("| File | Circuit | SHA-256 |");
    println!("|------|---------|---------|");
    for (file, desc, hash) in &rows {
        println!("| `{file}` | {desc} | `{hash}` |");
    }
}

/// Minimal hex encoder — avoids adding a dependency for one call site.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}
