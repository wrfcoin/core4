use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ark_bn254::{Bn254, Fr};
use ark_groth16::{PreparedVerifyingKey, ProvingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use sha2::{Digest, Sha256};
use wrfcoin_zk_circuits::circuits::confidential_transfer::ConfidentialTransferCircuit;
use wrfcoin_zk_circuits::circuits::range_proof::RangeProofCircuit;
use wrfcoin_zk_circuits::circuits::selective_disclosure::SelectiveDisclosureCircuit;
use wrfcoin_zk_circuits::circuits::weather_model::WeatherModelCircuit;
use wrfcoin_zk_circuits::error::ZkError;
use wrfcoin_zk_circuits::proving;
use wrfcoin_zk_circuits::types::{CircuitConfig, ZkProof};

use wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ZkServiceError {
    #[error("zk error: {0}")]
    Zk(#[from] ZkError),
    #[error("hex decode error: {0}")]
    HexDecode(#[from] hex::FromHexError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("key cache error: {0}")]
    KeyCache(String),
    /// The requested confidential-transfer circuit config is not one this node
    /// supports (#2038). Returned instead of triggering an unbounded on-demand
    /// Groth16 trusted setup for an arbitrary caller-chosen `(N, M)`. Maps to
    /// HTTP 422 at the route layer.
    #[error("unsupported circuit config: {0}")]
    UnsupportedConfig(String),
}

/// Confidential-transfer circuit configs this node supports, mirroring the
/// `supported_configs` advertised by `/zk/circuits`. Key loading/generation and
/// verification are restricted to these (#2038): without this bound a caller
/// could iterate arbitrary `(num_inputs, num_outputs)` pairs and force one full
/// Groth16 trusted setup + one persisted keyfile per novel pair (~2^64 space) —
/// an unauthenticated-reachable CPU/disk-exhaustion DoS. With it, at most the
/// 2×2 key is ever generated on demand (once, then cached), and disk holds at
/// most one file per supported config. Adding an entry here is a deliberate act
/// that authorises a one-time on-demand keygen for that circuit.
pub const SUPPORTED_CONFIDENTIAL_TRANSFER_CONFIGS: &[(usize, usize)] = &[(1, 1), (2, 2)];

/// Whether `config` is an advertised, supported confidential-transfer circuit.
pub fn is_supported_confidential_transfer_config(config: &CircuitConfig) -> bool {
    SUPPORTED_CONFIDENTIAL_TRANSFER_CONFIGS
        .iter()
        .any(|&(n, m)| config.num_inputs == n && config.num_outputs == m)
}

// ---------------------------------------------------------------------------
// Lazy key management (one-time generation per circuit type)
// ---------------------------------------------------------------------------

type KeyPair = (ProvingKey<Bn254>, PreparedVerifyingKey<Bn254>);

static RANGE_PROOF_KEYS: OnceLock<Result<KeyPair, String>> = OnceLock::new();
static RANGE_PROOF_32_KEYS: OnceLock<Result<KeyPair, String>> = OnceLock::new();
/// Per-config key cache for confidential transfer. Keys are loaded from disk
/// or generated on first use per (num_inputs, num_outputs) pair.
static CONFIDENTIAL_TRANSFER_KEYS: OnceLock<Mutex<HashMap<CircuitConfig, Arc<KeyPair>>>> =
    OnceLock::new();
static SELECTIVE_DISCLOSURE_KEYS_3: OnceLock<Result<KeyPair, String>> = OnceLock::new();
static WEATHER_MODEL_KEYS_2: OnceLock<Result<KeyPair, String>> = OnceLock::new();

/// A compile-time-embedded ZK key bundle (proving key + prepared verifying key).
///
/// # Provenance
///
/// These bundles are generated via single-party trusted setup (`OsRng`).
/// See `zk-params/PROVENANCE.md` for SHA-256 hashes and the full key rotation
/// plan.
///
/// # Key rotation (mainnet)
///
/// When MPC-ceremony-derived keys are available (see issues #717, #716):
///   1. Replace the `.bin` files in `native-chain/zk-params/`.
///   2. Update the SHA-256 hashes in `zk-params/PROVENANCE.md`.
///   3. Ship a new binary coordinated with a hard-fork activation height,
///      because proofs generated under old keys will not verify with the
///      new `PreparedVerifyingKey`.
///   4. The on-disk key cache (`$WRFCOIN_DATA_DIR/zk-keys/`) will auto-restore
///      from the new embedded bundle on first boot after upgrade.
struct EmbeddedKeyBundle {
    circuit_name: &'static str,
    bytes: &'static [u8],
}

const RANGE_PROOF_BUNDLE: EmbeddedKeyBundle = EmbeddedKeyBundle {
    circuit_name: "range-proof-64",
    bytes: include_bytes!("../zk-params/range-proof-64.bin"),
};

const CONFIDENTIAL_TRANSFER_BUNDLE: EmbeddedKeyBundle = EmbeddedKeyBundle {
    circuit_name: "confidential-transfer-1-1-64",
    bytes: include_bytes!("../zk-params/confidential-transfer-1-1-64.bin"),
};

const SELECTIVE_DISCLOSURE_BUNDLE: EmbeddedKeyBundle = EmbeddedKeyBundle {
    circuit_name: "selective-disclosure-3",
    bytes: include_bytes!("../zk-params/selective-disclosure-3.bin"),
};

const WEATHER_MODEL_BUNDLE: EmbeddedKeyBundle = EmbeddedKeyBundle {
    circuit_name: "weather-model-2",
    bytes: include_bytes!("../zk-params/weather-model-2.bin"),
};

fn default_data_dir() -> PathBuf {
    env::var_os("WRFCOIN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"))
}

fn zk_keys_dir() -> PathBuf {
    default_data_dir().join("zk-keys")
}

fn zk_key_path(file_name: String) -> PathBuf {
    zk_keys_dir().join(file_name)
}

fn transfer_key_path(config: &CircuitConfig) -> PathBuf {
    zk_keys_dir().join(format!("transfer-{}x{}.bin", config.num_inputs, config.num_outputs))
}

fn serialize_key_pair(key_pair: &KeyPair) -> Result<Vec<u8>, ZkServiceError> {
    let mut bytes = Vec::new();
    key_pair
        .0
        .serialize_compressed(&mut bytes)
        .map_err(|e| ZkServiceError::Serialization(e.to_string()))?;
    key_pair
        .1
        .serialize_compressed(&mut bytes)
        .map_err(|e| ZkServiceError::Serialization(e.to_string()))?;
    Ok(bytes)
}

fn deserialize_key_pair(bytes: &[u8]) -> Result<KeyPair, ZkServiceError> {
    let mut cursor = Cursor::new(bytes);
    let pk = ProvingKey::<Bn254>::deserialize_compressed(&mut cursor)
        .map_err(|e| ZkServiceError::Serialization(e.to_string()))?;
    let pvk = PreparedVerifyingKey::<Bn254>::deserialize_compressed(&mut cursor)
        .map_err(|e| ZkServiceError::Serialization(e.to_string()))?;
    if cursor.position() != bytes.len() as u64 {
        return Err(ZkServiceError::Serialization(
            "unexpected trailing bytes in persisted zk key file".to_string(),
        ));
    }
    // The serialized `PreparedVerifyingKey` embeds precomputed G2 line coefficients
    // whose encoding is NOT stable across arkworks majors (0.4 -> 0.6 deserializes
    // without error but yields wrong `gamma_g2_neg_pc`/`delta_g2_neg_pc`, so every
    // proof is rejected). Only the raw `VerifyingKey` is portable: fail closed unless
    // it matches the proving key's, then re-derive the prepared form locally.
    if pvk.vk != pk.vk {
        return Err(ZkServiceError::Serialization(
            "persisted prepared verifying key does not match its proving key".to_string(),
        ));
    }
    let pvk = ark_groth16::prepare_verifying_key(&pk.vk);
    Ok((pk, pvk))
}

fn write_key_pair(path: &Path, key_pair: &KeyPair) -> Result<(), ZkServiceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serialize_key_pair(key_pair)?;
    write_key_bytes(path, &bytes)
}

fn write_key_bytes(path: &Path, bytes: &[u8]) -> Result<(), ZkServiceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Write to a sibling temp file first, then rename atomically. This
    // prevents a crash between O_TRUNC and write completion from leaving a
    // zero-byte or partial key file that `load_or_restore_key_pair` would
    // silently treat as corrupt and overwrite.
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn bundle_fingerprint(bundle: &EmbeddedKeyBundle) -> String {
    hex::encode(Sha256::digest(bundle.bytes))
}

fn bundle_file_name(bundle: &EmbeddedKeyBundle) -> String {
    let fingerprint = bundle_fingerprint(bundle);
    format!("{}-{}.bin", bundle.circuit_name, &fingerprint[..16])
}

fn embedded_key_pair(bundle: &EmbeddedKeyBundle) -> Result<KeyPair, ZkServiceError> {
    deserialize_key_pair(bundle.bytes)
}

fn write_embedded_key_pair(path: &Path, bundle: &EmbeddedKeyBundle) -> Result<(), ZkServiceError> {
    // Validate the bundle loads, then persist its bytes verbatim so the restored
    // file is byte-identical to the embedded canonical parameters.
    embedded_key_pair(bundle)?;
    write_key_bytes(path, bundle.bytes)?;
    Ok(())
}

fn load_or_restore_key_pair(
    path: &Path,
    bundle: &EmbeddedKeyBundle,
) -> Result<KeyPair, ZkServiceError> {
    match fs::read(path) { // key-file-exempt: ark-groth16 proving/verifying KeyPair (public trusted-setup parameters), not a secret auth key
        Ok(bytes) => {
            // Accept any correctly serialized key pair on disk — including
            // ceremony-derived keys and keys from different circuit versions.
            // Only fall back to the embedded bundle if the bytes are corrupt
            // (deserialization fails).
            match deserialize_key_pair(&bytes) {
                Ok(key_pair) => Ok(key_pair),
                Err(_) => {
                    log::warn!(
                        "zk key file at '{}' failed to deserialize (corrupt or partial write); \
                         restoring from embedded canonical bundle for circuit '{}'",
                        path.display(),
                        bundle.circuit_name,
                    );
                    write_embedded_key_pair(path, bundle)?;
                    embedded_key_pair(bundle)
                }
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            write_embedded_key_pair(path, bundle)?;
            embedded_key_pair(bundle)
        },
        Err(err) => Err(ZkServiceError::Io(err)),
    }
}

fn cached_key_pair(
    cache: &'static OnceLock<Result<KeyPair, String>>,
    bundle: &'static EmbeddedKeyBundle,
) -> Result<&'static KeyPair, ZkServiceError> {
    cache
        .get_or_init(|| {
            load_or_restore_key_pair(&zk_key_path(bundle_file_name(bundle)), bundle)
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| ZkServiceError::KeyCache(e.clone()))
}

fn range_proof_keys() -> Result<&'static KeyPair, ZkServiceError> {
    cached_key_pair(&RANGE_PROOF_KEYS, &RANGE_PROOF_BUNDLE)
}

/// Return an `Arc`-wrapped key pair for the given confidential-transfer config.
///
/// For 1×1 transfers the embedded canonical bundle is used as a fallback
/// (fast, no key-generation on first boot).  For other configs the keys are
/// generated on first use and persisted to
/// `$WRFCOIN_DATA_DIR/zk-keys/transfer-NxM.bin`.
fn confidential_transfer_keys(config: CircuitConfig) -> Result<Arc<KeyPair>, ZkServiceError> {
    // #2038: the single choke point for confidential-transfer key material.
    // Reject any non-advertised (N, M) BEFORE touching the cache, disk, or a
    // Groth16 trusted setup, so neither the verify path nor the generate path
    // can be driven to unbounded keygen/disk growth by an arbitrary caller-chosen
    // circuit size. Only the supported configs (1×1 embedded, 2×2 generated once)
    // are ever materialised.
    if !is_supported_confidential_transfer_config(&config) {
        return Err(ZkServiceError::UnsupportedConfig(format!(
            "confidential-transfer {}x{} is not supported; supported configs: {:?}",
            config.num_inputs, config.num_outputs, SUPPORTED_CONFIDENTIAL_TRANSFER_CONFIGS
        )));
    }
    let map_mutex =
        CONFIDENTIAL_TRANSFER_KEYS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = map_mutex
        .lock()
        .map_err(|e| ZkServiceError::KeyCache(e.to_string()))?;

    if !map.contains_key(&config) {
        let pair = if config.num_inputs == 1 && config.num_outputs == 1 {
            // 1×1: use the embedded bundle so the first boot is instant.
            load_or_restore_key_pair(
                &zk_key_path(bundle_file_name(&CONFIDENTIAL_TRANSFER_BUNDLE)),
                &CONFIDENTIAL_TRANSFER_BUNDLE,
            )?
        } else {
            // NxM: generate dynamically and persist to disk.
            let path = transfer_key_path(&config);
            // Attempt to load from disk first; generate (and persist) on any miss or corruption.
            let loaded = fs::read(&path) // key-file-exempt: ZK circuit KeyPair (public parameters), not a secret auth key
                .ok()
                .and_then(|b| deserialize_key_pair(&b).ok());
            if let Some(pair) = loaded {
                pair
            } else {
                let generated = proving::generate_keys(ConfidentialTransferCircuit::blank(
                    config.num_inputs,
                    config.num_outputs,
                    64,
                ))
                .map_err(|e| ZkServiceError::Zk(ZkError::KeyGenerationFailed(e.to_string())))?;
                let _ = write_key_pair(&path, &generated);
                generated
            }
        };
        map.insert(config, Arc::new(pair));
    }

    Ok(Arc::clone(map.get(&config).unwrap()))
}

/// Pre-warm the 1×1 confidential-transfer key at startup to avoid first-call
/// latency hitting an interactive request.
pub fn prewarm_confidential_transfer_keys() -> Result<(), ZkServiceError> {
    confidential_transfer_keys(CircuitConfig::new(1, 1))?;
    Ok(())
}

fn selective_disclosure_keys_3() -> Result<&'static KeyPair, ZkServiceError> {
    cached_key_pair(&SELECTIVE_DISCLOSURE_KEYS_3, &SELECTIVE_DISCLOSURE_BUNDLE)
}

fn weather_model_keys_2() -> Result<&'static KeyPair, ZkServiceError> {
    cached_key_pair(&WEATHER_MODEL_KEYS_2, &WEATHER_MODEL_BUNDLE)
}

// ---------------------------------------------------------------------------
// Hex bridge
// ---------------------------------------------------------------------------

pub fn proof_to_hex(proof: &ZkProof) -> String {
    hex::encode(&proof.proof_bytes)
}

pub fn proof_from_hex(hex_str: &str, circuit_id: &str) -> Result<ZkProof, ZkServiceError> {
    let proof_bytes = hex::decode(hex_str)?;
    Ok(ZkProof {
        proof_bytes,
        circuit_id: circuit_id.to_string(),
    })
}

fn fr_to_hex(f: &Fr) -> String {
    let mut buf = Vec::new();
    f.serialize_compressed(&mut buf)
        .expect("Fr serialization cannot fail");
    hex::encode(buf)
}

fn fr_from_hex(hex_str: &str) -> Result<Fr, ZkServiceError> {
    let bytes = hex::decode(hex_str)?;
    Fr::deserialize_compressed(&bytes[..]).map_err(|e| ZkServiceError::Serialization(e.to_string()))
}

// ---------------------------------------------------------------------------
// Range proof API
// ---------------------------------------------------------------------------

/// Generate a range proof for `value` with the given hex-encoded blinding factor.
/// Returns `(proof_hex, commitment_hex)`.
pub fn generate_range_proof(
    value: u64,
    blinding_factor_hex: &str,
) -> Result<(String, String), ZkServiceError> {
    let blinding = fr_from_hex(blinding_factor_hex)?;
    let commitment = RangeProofCircuit::compute_commitment(value, blinding);

    let (pk, _) = range_proof_keys()?;
    let circuit = RangeProofCircuit::new(commitment, Fr::from(value), blinding, 64);
    let proof = proving::prove(circuit, pk, "range-proof")?;

    Ok((proof_to_hex(&proof), fr_to_hex(&commitment)))
}

/// Verify a range proof against a commitment.
pub fn verify_range_proof(proof_hex: &str, commitment_hex: &str) -> Result<bool, ZkServiceError> {
    let proof = proof_from_hex(proof_hex, "range-proof")?;
    let commitment = fr_from_hex(commitment_hex)?;
    let (_, pvk) = range_proof_keys()?;
    Ok(proving::verify(&proof, pvk, &[commitment])?)
}

// ---------------------------------------------------------------------------
// 32-bit range proof API (mobile-optimized, ~49% fewer constraints)
// ---------------------------------------------------------------------------

fn range_proof_32_keys() -> Result<&'static KeyPair, ZkServiceError> {
    RANGE_PROOF_32_KEYS
        .get_or_init(|| {
            // No embedded bundle for 32-bit — generate lazily and persist to disk.
            let path = zk_key_path("range-proof-32.bin".to_string());
            let loaded = fs::read(&path) // key-file-exempt: ZK circuit KeyPair (public parameters), not a secret auth key
                .ok()
                .and_then(|b| deserialize_key_pair(&b).ok());
            if let Some(pair) = loaded {
                Ok(pair)
            } else {
                log::info!("generating 32-bit range proof keys (first use)");
                let pair = proving::generate_keys(RangeProofCircuit::blank(32))
                    .map_err(|e| e.to_string())?;
                let _ = write_key_pair(&path, &pair);
                Ok(pair)
            }
        })
        .as_ref()
        .map_err(|e| ZkServiceError::KeyCache(e.clone()))
}

/// Generate a 32-bit range proof for `value` (must fit in u32).
/// Uses ~49% fewer constraints than the 64-bit variant — optimized for
/// mobile weather data where values (temperature, pressure, humidity) fit in u32.
/// Returns `(proof_hex, commitment_hex)`.
pub fn generate_range_proof_32(
    value: u32,
    blinding_factor_hex: &str,
) -> Result<(String, String), ZkServiceError> {
    let blinding = fr_from_hex(blinding_factor_hex)?;
    let commitment = RangeProofCircuit::compute_commitment(value as u64, blinding);

    let (pk, _) = range_proof_32_keys()?;
    let circuit = RangeProofCircuit::new(commitment, Fr::from(value as u64), blinding, 32);
    let proof = proving::prove(circuit, pk, "range-proof-32")?;

    Ok((proof_to_hex(&proof), fr_to_hex(&commitment)))
}

/// Verify a 32-bit range proof against a commitment.
pub fn verify_range_proof_32(
    proof_hex: &str,
    commitment_hex: &str,
) -> Result<bool, ZkServiceError> {
    let proof = proof_from_hex(proof_hex, "range-proof-32")?;
    let commitment = fr_from_hex(commitment_hex)?;
    let (_, pvk) = range_proof_32_keys()?;
    Ok(proving::verify(&proof, pvk, &[commitment])?)
}

// ---------------------------------------------------------------------------
// Batch verification API (#624)
// ---------------------------------------------------------------------------

/// Verify multiple range proofs in a single batch for improved performance.
///
/// Uses randomized Groth16 batch verification with 1 `final_exponentiation`
/// instead of N, yielding ~3-5x speedup for batches of 10+ proofs.
pub fn batch_verify_range_proofs(
    proof_commitment_pairs: &[(&str, &str)],
) -> Result<bool, ZkServiceError> {
    if proof_commitment_pairs.is_empty() {
        return Ok(true);
    }

    let (_, pvk) = range_proof_keys()?;

    let proofs: Vec<ZkProof> = proof_commitment_pairs
        .iter()
        .map(|(proof_hex, _)| proof_from_hex(proof_hex, "range-proof"))
        .collect::<Result<_, _>>()?;

    let commitments: Vec<Fr> = proof_commitment_pairs
        .iter()
        .map(|(_, commitment_hex)| fr_from_hex(commitment_hex))
        .collect::<Result<_, _>>()?;

    let proof_refs: Vec<&ZkProof> = proofs.iter().collect();
    let inputs: Vec<Vec<Fr>> = commitments.iter().map(|c| vec![*c]).collect();
    let input_refs: Vec<&[Fr]> = inputs.iter().map(|v| v.as_slice()).collect();

    Ok(proving::batch_verify(&proof_refs, pvk, &input_refs)?)
}

// ---------------------------------------------------------------------------
// Confidential transfer API (arbitrary num_inputs × num_outputs)
// ---------------------------------------------------------------------------

pub struct ConfidentialTransferInputs {
    pub input_values: Vec<u64>,
    pub input_blindings: Vec<String>,
    pub output_values: Vec<u64>,
    pub output_blindings: Vec<String>,
    pub fee: u64,
}

/// Generate a confidential transfer proof.
/// Supports arbitrary num_inputs × num_outputs (≥ 1 each).
/// Returns `(proof_hex, input_commitment_hex, output_commitment_hex)`.
pub fn generate_confidential_transfer_proof(
    inputs: &ConfidentialTransferInputs,
) -> Result<(String, String, String), ZkServiceError> {
    let num_inputs = inputs.input_values.len();
    let num_outputs = inputs.output_values.len();
    if num_inputs == 0 || num_outputs == 0 {
        return Err(ZkServiceError::Zk(ZkError::InvalidInput(
            "confidential transfer requires at least 1 input and 1 output".into(),
        )));
    }
    if inputs.input_blindings.len() != num_inputs
        || inputs.output_blindings.len() != num_outputs
    {
        return Err(ZkServiceError::Zk(ZkError::InvalidInput(
            "blinding factors count must match values count".into(),
        )));
    }

    let config = CircuitConfig::new(num_inputs, num_outputs);
    let circuit_id = config.circuit_id("confidential-transfer");

    let input_vals: Vec<Fr> = inputs.input_values.iter().map(|v| Fr::from(*v)).collect();
    let input_blinds: Vec<Fr> = inputs
        .input_blindings
        .iter()
        .map(|h| fr_from_hex(h))
        .collect::<Result<_, _>>()?;
    let output_vals: Vec<Fr> = inputs.output_values.iter().map(|v| Fr::from(*v)).collect();
    let output_blinds: Vec<Fr> = inputs
        .output_blindings
        .iter()
        .map(|h| fr_from_hex(h))
        .collect::<Result<_, _>>()?;
    let fee = Fr::from(inputs.fee);

    let input_comm = ConfidentialTransferCircuit::compute_commitment(&input_vals, &input_blinds);
    let output_comm = ConfidentialTransferCircuit::compute_commitment(&output_vals, &output_blinds);

    let keys = confidential_transfer_keys(config)?;
    let (pk, _) = &*keys;
    let circuit = ConfidentialTransferCircuit::new(
        input_comm,
        output_comm,
        fee,
        input_vals,
        input_blinds,
        output_vals,
        output_blinds,
        64,
    );
    let proof = proving::prove(circuit, pk, &circuit_id)?;

    Ok((proof_to_hex(&proof), fr_to_hex(&input_comm), fr_to_hex(&output_comm)))
}

/// Verify a confidential transfer proof.
pub fn verify_confidential_transfer_proof(
    proof_hex: &str,
    input_commitment_hex: &str,
    output_commitment_hex: &str,
    fee: u64,
    num_inputs: usize,
    num_outputs: usize,
) -> Result<bool, ZkServiceError> {
    let config = CircuitConfig::new(num_inputs, num_outputs);
    // #2038: fail fast on an unsupported circuit size — before parsing the
    // (attacker-supplied) proof/commitments and, crucially, before
    // `confidential_transfer_keys` could ever reach a trusted setup. Rejecting
    // here means an unsupported (N, M) does zero cryptographic work.
    if !is_supported_confidential_transfer_config(&config) {
        return Err(ZkServiceError::UnsupportedConfig(format!(
            "confidential-transfer {}x{} is not supported; supported configs: {:?}",
            config.num_inputs, config.num_outputs, SUPPORTED_CONFIDENTIAL_TRANSFER_CONFIGS
        )));
    }
    let circuit_id = config.circuit_id("confidential-transfer");
    let proof = proof_from_hex(proof_hex, &circuit_id)?;
    let input_comm = fr_from_hex(input_commitment_hex)?;
    let output_comm = fr_from_hex(output_commitment_hex)?;
    let fee_fr = Fr::from(fee);
    let keys = confidential_transfer_keys(config)?;
    let (_, pvk) = &*keys;
    Ok(proving::verify(&proof, pvk, &[input_comm, output_comm, fee_fr])?)
}

// ---------------------------------------------------------------------------
// Selective disclosure API
// ---------------------------------------------------------------------------

/// Generate a selective disclosure proof.
/// Returns `(proof_hex, commitment_hex)`.
pub fn generate_selective_disclosure_proof(
    field_values: &[u64],
) -> Result<(String, String), ZkServiceError> {
    if field_values.len() != 3 {
        return Err(ZkServiceError::Zk(ZkError::InvalidInput(
            "selective disclosure currently requires exactly 3 fields".into(),
        )));
    }
    let fields: Vec<Fr> = field_values.iter().map(|v| Fr::from(*v)).collect();
    let commitment = SelectiveDisclosureCircuit::compute_commitment(&fields);

    let (pk, _) = selective_disclosure_keys_3()?;
    let circuit = SelectiveDisclosureCircuit::new(commitment, fields);
    let proof = proving::prove(circuit, pk, "selective-disclosure")?;

    Ok((proof_to_hex(&proof), fr_to_hex(&commitment)))
}

/// Verify a selective disclosure proof against a commitment.
pub fn verify_selective_disclosure_proof(
    proof_hex: &str,
    commitment_hex: &str,
) -> Result<bool, ZkServiceError> {
    let proof = proof_from_hex(proof_hex, "selective-disclosure")?;
    let commitment = fr_from_hex(commitment_hex)?;
    let (_, pvk) = selective_disclosure_keys_3()?;
    Ok(proving::verify(&proof, pvk, &[commitment])?)
}

// ---------------------------------------------------------------------------
// Weather model API
// ---------------------------------------------------------------------------

/// Generate a weather model execution proof.
pub fn generate_weather_model_proof(
    input_hash_hex: &str,
    output_hash_hex: &str,
    model_id: u64,
    time_secs: u64,
    samples: &[u64],
) -> Result<String, ZkServiceError> {
    let input_hash = fr_from_hex(input_hash_hex)?;
    let output_hash = fr_from_hex(output_hash_hex)?;
    let model_id_fr = Fr::from(model_id);
    let time_fr = Fr::from(time_secs);
    let sample_frs: Vec<Fr> = samples.iter().map(|v| Fr::from(*v)).collect();

    let (pk, _) = weather_model_keys_2()?;
    let circuit =
        WeatherModelCircuit::new(input_hash, output_hash, model_id_fr, time_fr, sample_frs);
    let proof = proving::prove(circuit, pk, "weather-model")?;

    Ok(proof_to_hex(&proof))
}

/// Verify a weather model execution proof.
pub fn verify_weather_model_proof(
    proof_hex: &str,
    input_hash_hex: &str,
    output_hash_hex: &str,
    model_id: u64,
    time_secs: u64,
) -> Result<bool, ZkServiceError> {
    let proof = proof_from_hex(proof_hex, "weather-model")?;
    let input_hash = fr_from_hex(input_hash_hex)?;
    let output_hash = fr_from_hex(output_hash_hex)?;
    let model_id_fr = Fr::from(model_id);
    let time_fr = Fr::from(time_secs);

    let (_, pvk) = weather_model_keys_2()?;
    Ok(proving::verify(
        &proof,
        pvk,
        &[input_hash, output_hash, model_id_fr, time_fr],
    )?)
}

// ---------------------------------------------------------------------------
// ArkworksZkVerifier — implements the shared-protocol ZkVerifier trait
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ArkworksZkVerifier;

impl ZkVerifier for ArkworksZkVerifier {
    fn verify_range_proof(&self, proof_hex: &str, commitment_hex: &str) -> Result<bool, String> {
        verify_range_proof(proof_hex, commitment_hex).map_err(|e| e.to_string())
    }

    fn verify_confidential_transfer(
        &self,
        proof_hex: &str,
        input_commitment_hex: &str,
        output_commitment_hex: &str,
        fee: u64,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<bool, String> {
        verify_confidential_transfer_proof(
            proof_hex,
            input_commitment_hex,
            output_commitment_hex,
            fee,
            num_inputs,
            num_outputs,
        )
        .map_err(|e| e.to_string())
    }

    fn verify_selective_disclosure(
        &self,
        proof_hex: &str,
        commitment_hex: &str,
    ) -> Result<bool, String> {
        verify_selective_disclosure_proof(proof_hex, commitment_hex).map_err(|e| e.to_string())
    }

    fn verify_weather_model(
        &self,
        proof_hex: &str,
        input_hash_hex: &str,
        output_hash_hex: &str,
        model_id: u64,
        time_secs: u64,
    ) -> Result<bool, String> {
        verify_weather_model_proof(
            proof_hex,
            input_hash_hex,
            output_hash_hex,
            model_id,
            time_secs,
        )
        .map_err(|e| e.to_string())
    }

    fn verify_batch_proof(&self, proof_hex: &str, commitment_hex: &str) -> Result<bool, String> {
        verify_range_proof(proof_hex, commitment_hex).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Circuit info
// ---------------------------------------------------------------------------

pub fn available_circuits() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "id": "range-proof",
            "description": "Proves a committed value lies in [0, 2^64)",
            "public_inputs": ["commitment"],
            "curve": "BN254",
            "proof_system": "Groth16",
        }),
        serde_json::json!({
            "id": "confidential-transfer",
            "description": "Proves a confidential transfer is balanced (inputs = outputs + fee). \
                            Only the configs in supported_configs are offered (#2038); \
                            circuit_id: 'confidential-transfer-NxM'.",
            "public_inputs": ["input_commitment", "output_commitment", "fee"],
            "curve": "BN254",
            "proof_system": "Groth16",
            "supported_configs": [{"num_inputs": 1, "num_outputs": 1}, {"num_inputs": 2, "num_outputs": 2}],
        }),
        serde_json::json!({
            "id": "selective-disclosure",
            "description": "Proves knowledge of field preimages matching a commitment",
            "public_inputs": ["commitment"],
            "curve": "BN254",
            "proof_system": "Groth16",
        }),
        serde_json::json!({
            "id": "weather-model",
            "description": "Proves WRF weather model execution integrity",
            "public_inputs": ["input_hash", "output_hash", "model_id", "execution_time_secs"],
            "curve": "BN254",
            "proof_system": "Groth16",
        }),
    ]
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // The serialized PreparedVerifyingKey is not portable across arkworks majors
    // (0.4 -> 0.6 deserializes cleanly into wrong G2 line coefficients), so only the
    // raw VerifyingKey is trusted and the prepared form is re-derived locally.
    fn split_bundle(bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut c = Cursor::new(bytes);
        let _ = ProvingKey::<Bn254>::deserialize_compressed(&mut c).unwrap();
        let at = c.position() as usize;
        (bytes[..at].to_vec(), bytes[at..].to_vec())
    }

    #[test]
    fn mismatched_embedded_pvk_is_rejected_fail_closed() {
        let (pk_a, _) = split_bundle(RANGE_PROOF_BUNDLE.bytes);
        let (_, pvk_b) = split_bundle(SELECTIVE_DISCLOSURE_BUNDLE.bytes);
        let mut spliced = pk_a;
        spliced.extend_from_slice(&pvk_b);
        let err = deserialize_key_pair(&spliced).expect_err("pk/pvk from different circuits must be rejected");
        assert!(err.to_string().contains("does not match its proving key"), "{err}");
    }

    #[test]
    fn corrupted_embedded_pvk_vk_is_rejected_fail_closed() {
        let (pk, mut pvk) = split_bundle(SELECTIVE_DISCLOSURE_BUNDLE.bytes);
        // First bytes of the serialized pvk are `vk.alpha_g1`; flip one.
        pvk[0] ^= 0x01;
        let mut bytes = pk;
        bytes.extend_from_slice(&pvk);
        assert!(deserialize_key_pair(&bytes).is_err(), "tampered pvk.vk must not load");
    }

    #[test]
    fn embedded_bundles_load_and_verify_with_rederived_pvk() {
        for b in [&RANGE_PROOF_BUNDLE, &CONFIDENTIAL_TRANSFER_BUNDLE, &SELECTIVE_DISCLOSURE_BUNDLE, &WEATHER_MODEL_BUNDLE] {
            let (pk, pvk) = embedded_key_pair(b).expect(b.circuit_name);
            assert!(pvk.vk == pk.vk, "{}", b.circuit_name);
        }
    }

    fn test_blinding_hex(val: u64) -> String {
        fr_to_hex(&Fr::from(val))
    }

    /// The output hash `WeatherModelCircuit` actually enforces:
    /// `Poseidon(state_samples || model_id || input_hash)`.
    ///
    /// Fixtures used to hard-code the pre-#837 linear sum here, which stopped
    /// satisfying the circuit when it moved to Poseidon. Deriving it from the
    /// circuit's own hash keeps the two from drifting apart again (#2283).
    fn weather_output_hash(samples: &[u64], model_id: u64, input_hash: u64) -> String {
        use wrfcoin_zk_circuits::circuits::poseidon_config::poseidon_hash;
        let mut fields: Vec<Fr> = samples.iter().map(|s| Fr::from(*s)).collect();
        fields.push(Fr::from(model_id));
        fields.push(Fr::from(input_hash));
        fr_to_hex(&poseidon_hash(&fields))
    }

    // #2038 — confidential-transfer key material is bounded to the advertised
    // supported configs so a caller cannot force unbounded Groth16 trusted-setup
    // / disk growth by iterating arbitrary (num_inputs, num_outputs).

    #[test]
    fn supported_configs_match_advertised_2038() {
        assert!(is_supported_confidential_transfer_config(&CircuitConfig::new(1, 1)));
        assert!(is_supported_confidential_transfer_config(&CircuitConfig::new(2, 2)));
        // Anything else is rejected — including the previously-allowed up-to-16 range.
        for (n, m) in [(1usize, 2usize), (2, 1), (3, 3), (4, 4), (16, 16), (8, 2)] {
            assert!(
                !is_supported_confidential_transfer_config(&CircuitConfig::new(n, m)),
                "{n}x{m} must not be a supported config"
            );
        }
    }

    #[test]
    fn unsupported_config_rejected_before_keygen_2038() {
        // The private key-loading choke point must reject an unsupported config
        // with `UnsupportedConfig` and NOT trigger a trusted setup or disk write.
        let err = confidential_transfer_keys(CircuitConfig::new(3, 3))
            .expect_err("3x3 must be rejected, not generated");
        assert!(
            matches!(err, ZkServiceError::UnsupportedConfig(_)),
            "expected UnsupportedConfig, got {err:?}"
        );
    }

    #[test]
    fn verify_unsupported_config_fails_fast_without_work_2038() {
        // The public verify path rejects an unsupported (N, M) before parsing the
        // (here deliberately garbage) proof/commitments — proving the DoS trigger
        // is closed regardless of proof validity.
        let err = verify_confidential_transfer_proof("00", "00", "00", 0, 5, 7)
            .expect_err("5x7 verify must be rejected");
        assert!(
            matches!(err, ZkServiceError::UnsupportedConfig(_)),
            "expected UnsupportedConfig (fast-fail before proof parse), got {err:?}"
        );
    }

    #[test]
    fn hex_roundtrip_proof() {
        let proof = ZkProof {
            proof_bytes: vec![1, 2, 3, 4, 5],
            circuit_id: "test".to_string(),
        };
        let hex_str = proof_to_hex(&proof);
        let recovered = proof_from_hex(&hex_str, "test").unwrap();
        assert_eq!(proof.proof_bytes, recovered.proof_bytes);
    }

    #[test]
    fn hex_roundtrip_fr() {
        let val = Fr::from(12345u64);
        let hex_str = fr_to_hex(&val);
        let recovered = fr_from_hex(&hex_str).unwrap();
        assert_eq!(val, recovered);
    }

    #[test]
    fn range_proof_generate_and_verify() {
        let blinding_hex = test_blinding_hex(42);
        let (proof_hex, commitment_hex) = generate_range_proof(1000, &blinding_hex).unwrap();
        let result = verify_range_proof(&proof_hex, &commitment_hex).unwrap();
        assert!(result, "valid range proof should verify");
    }

    #[test]
    fn range_proof_wrong_commitment_fails() {
        let blinding_hex = test_blinding_hex(42);
        let (proof_hex, _) = generate_range_proof(1000, &blinding_hex).unwrap();
        let wrong_commitment = RangeProofCircuit::compute_commitment(9999, Fr::from(42u64));
        let wrong_hex = fr_to_hex(&wrong_commitment);
        let result = verify_range_proof(&proof_hex, &wrong_hex).unwrap();
        assert!(!result, "wrong commitment should fail");
    }

    #[test]
    fn confidential_transfer_generate_and_verify() {
        // Blinding sums must be conserved across inputs and outputs (#786):
        // the circuit enforces sum(input_blindings) == sum(output_blindings).
        let inputs = ConfidentialTransferInputs {
            input_values: vec![100],
            input_blindings: vec![test_blinding_hex(42)],
            output_values: vec![90],
            output_blindings: vec![test_blinding_hex(42)],
            fee: 10,
        };
        let (proof_hex, input_comm_hex, output_comm_hex) =
            generate_confidential_transfer_proof(&inputs).unwrap();
        let result =
            verify_confidential_transfer_proof(&proof_hex, &input_comm_hex, &output_comm_hex, 10, 1, 1)
                .unwrap();
        assert!(result, "balanced transfer should verify");
    }

    #[test]
    fn confidential_transfer_wrong_fee_fails() {
        // Blinding sums conserved (#786) so that proof generation succeeds and
        // this test actually reaches the rejection it exists to assert.
        let inputs = ConfidentialTransferInputs {
            input_values: vec![100],
            input_blindings: vec![test_blinding_hex(42)],
            output_values: vec![90],
            output_blindings: vec![test_blinding_hex(42)],
            fee: 10,
        };
        let (proof_hex, input_comm_hex, output_comm_hex) =
            generate_confidential_transfer_proof(&inputs).unwrap();
        let result =
            verify_confidential_transfer_proof(&proof_hex, &input_comm_hex, &output_comm_hex, 5, 1, 1)
                .unwrap();
        assert!(!result, "wrong fee should fail");
    }

    #[test]
    fn confidential_transfer_rejects_zero_inputs() {
        let inputs = ConfidentialTransferInputs {
            input_values: vec![],
            input_blindings: vec![],
            output_values: vec![90],
            output_blindings: vec![test_blinding_hex(10)],
            fee: 10,
        };
        let result = generate_confidential_transfer_proof(&inputs);
        assert!(result.is_err(), "should reject 0 inputs");
    }

    #[test]
    fn confidential_transfer_2x2_roundtrip() {
        // Blinding sums conserved (#786): inputs 10+20 == outputs 15+15.
        let inputs = ConfidentialTransferInputs {
            input_values: vec![60, 40],
            input_blindings: vec![test_blinding_hex(10), test_blinding_hex(20)],
            output_values: vec![50, 45],
            output_blindings: vec![test_blinding_hex(15), test_blinding_hex(15)],
            fee: 5,
        };
        let (proof_hex, input_comm_hex, output_comm_hex) =
            generate_confidential_transfer_proof(&inputs).unwrap();
        let result =
            verify_confidential_transfer_proof(&proof_hex, &input_comm_hex, &output_comm_hex, 5, 2, 2)
                .unwrap();
        assert!(result, "2x2 transfer should verify");
    }

    #[test]
    fn confidential_transfer_wrong_config_fails() {
        // Blinding sums conserved (#786) so the 2x2 proof is genuinely produced
        // and the assertion below tests key/config mismatch, not proof failure.
        let inputs = ConfidentialTransferInputs {
            input_values: vec![60, 40],
            input_blindings: vec![test_blinding_hex(10), test_blinding_hex(20)],
            output_values: vec![50, 45],
            output_blindings: vec![test_blinding_hex(15), test_blinding_hex(15)],
            fee: 5,
        };
        let (proof_hex, input_comm_hex, output_comm_hex) =
            generate_confidential_transfer_proof(&inputs).unwrap();
        // 2x2 proof verified with 1x1 key must NOT succeed
        let result = verify_confidential_transfer_proof(
            &proof_hex, &input_comm_hex, &output_comm_hex, 5, 1, 1,
        );
        match result {
            Ok(true) => panic!("2x2 proof must not verify with 1x1 key"),
            _ => {} // Ok(false) or Err(_) are both acceptable failures
        }
    }

    #[test]
    fn selective_disclosure_generate_and_verify() {
        let (proof_hex, commitment_hex) =
            generate_selective_disclosure_proof(&[42, 100, 7]).unwrap();
        let result = verify_selective_disclosure_proof(&proof_hex, &commitment_hex).unwrap();
        assert!(result, "valid selective disclosure should verify");
    }

    #[test]
    fn selective_disclosure_wrong_commitment_fails() {
        let (proof_hex, _) = generate_selective_disclosure_proof(&[42, 100, 7]).unwrap();
        let wrong = Fr::from(999u64);
        let wrong_hex = fr_to_hex(&wrong);
        let result = verify_selective_disclosure_proof(&proof_hex, &wrong_hex).unwrap();
        assert!(!result, "wrong commitment should fail");
    }

    #[test]
    fn weather_model_generate_and_verify() {
        let input_hash = test_blinding_hex(100);
        let output_hash = weather_output_hash(&[10, 20], 1, 100);
        let proof_hex =
            generate_weather_model_proof(&input_hash, &output_hash, 1, 3600, &[10, 20]).unwrap();
        let result =
            verify_weather_model_proof(&proof_hex, &input_hash, &output_hash, 1, 3600).unwrap();
        assert!(result, "valid weather proof should verify");
    }

    #[test]
    fn weather_model_wrong_inputs_fails() {
        let input_hash = test_blinding_hex(100);
        let output_hash = weather_output_hash(&[10, 20], 1, 100);
        let proof_hex =
            generate_weather_model_proof(&input_hash, &output_hash, 1, 3600, &[10, 20]).unwrap();
        let result =
            verify_weather_model_proof(&proof_hex, &input_hash, &output_hash, 1, 3601).unwrap();
        assert!(!result, "wrong time should fail");
    }

    #[test]
    fn available_circuits_returns_four() {
        let circuits = available_circuits();
        assert_eq!(circuits.len(), 4);
    }

    #[test]
    fn arkworks_verifier_range_proof() {
        let verifier = ArkworksZkVerifier;
        let blinding_hex = test_blinding_hex(99);
        let (proof_hex, commitment_hex) = generate_range_proof(500, &blinding_hex).unwrap();
        let result = verifier
            .verify_range_proof(&proof_hex, &commitment_hex)
            .unwrap();
        assert!(result);
    }

    #[test]
    fn range_proof_32_generate_and_verify() {
        let blinding_hex = test_blinding_hex(42);
        let (proof_hex, commitment_hex) = generate_range_proof_32(1000, &blinding_hex).unwrap();
        let result = verify_range_proof_32(&proof_hex, &commitment_hex).unwrap();
        assert!(result, "valid 32-bit range proof should verify");
    }

    #[test]
    fn range_proof_32_wrong_commitment_fails() {
        let blinding_hex = test_blinding_hex(42);
        let (proof_hex, _) = generate_range_proof_32(1000, &blinding_hex).unwrap();
        let wrong_commitment = RangeProofCircuit::compute_commitment(9999, Fr::from(42u64));
        let wrong_hex = fr_to_hex(&wrong_commitment);
        let result = verify_range_proof_32(&proof_hex, &wrong_hex).unwrap();
        assert!(!result, "wrong commitment should fail");
    }

    #[test]
    fn batch_verify_range_proofs_roundtrip() {
        let blinding1 = test_blinding_hex(42);
        let blinding2 = test_blinding_hex(99);
        let (proof1, comm1) = generate_range_proof(1000, &blinding1).unwrap();
        let (proof2, comm2) = generate_range_proof(2000, &blinding2).unwrap();

        let pairs = vec![
            (proof1.as_str(), comm1.as_str()),
            (proof2.as_str(), comm2.as_str()),
        ];
        let result = batch_verify_range_proofs(&pairs).unwrap();
        assert!(result, "valid batch should verify");
    }

    #[test]
    fn batch_verify_range_proofs_one_bad_fails() {
        let blinding1 = test_blinding_hex(42);
        let (proof1, comm1) = generate_range_proof(1000, &blinding1).unwrap();
        let wrong = fr_to_hex(&Fr::from(9999u64));

        let pairs = vec![
            (proof1.as_str(), comm1.as_str()),
            (proof1.as_str(), wrong.as_str()),
        ];
        let result = batch_verify_range_proofs(&pairs).unwrap();
        assert!(!result, "batch with invalid proof should fail");
    }
}
