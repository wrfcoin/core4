//! Blockchain types for forecast verification on the WRFCoin network.
//!
//! Contains [`ForecastArtifactManifest`] and [`ForecastVerificationReceipt`] structs
//! along with ed25519 signing infrastructure for deterministic forecast verification.

use ed25519_dalek::{Signature as Ed25519Signature, Verifier, VerifyingKey};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use thiserror::Error;

use super::ForecastAccuracyMetrics;

/// Version tag embedded in every canonical signing message.
const SIGNING_SCHEME_VERSION: &str = "wrfcoin-sign-v1";
/// Env var naming the chain id folded into forecast signing preimages (#2034).
pub const CHAIN_ID_ENV: &str = "WRFCOIN_CHAIN_ID";
/// Env var naming the network id folded into forecast signing preimages (#2034).
pub const NETWORK_ID_ENV: &str = "WRFCOIN_NETWORK_ID";

/// The chain and network a receipt is signed and verified under (#3064).
///
/// Every canonical signing message folds these in, so a receipt signed under
/// one binding never verifies under another — that is the cross-network replay
/// barrier. What this type removes is the *implicit* binding: the previous
/// resolver substituted `wrfcoin-main` / `mainnet` whenever the environment
/// was unset, so a scorer that forgot to export `WRFCOIN_CHAIN_ID` signed a
/// perfectly well-formed mainnet receipt that the testnet node then refused,
/// and the forecaster's payout went to zero with one `WARN` line as the only
/// signal. Now there is no default: [`ChainBinding::from_env`] fails closed,
/// naming the variable, and the no-argument wrappers below never fall back to
/// mainnet.
///
/// The node itself is unaffected: it refuses to start without a declared
/// identity (#2713), so on a node both variables are always set and every
/// message here is byte-identical to what it was. Off-node signers get an
/// error where they used to get silence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainBinding {
    chain_id: std::borrow::Cow<'static, str>,
    network_id: std::borrow::Cow<'static, str>,
}

/// Missing or empty signing-domain configuration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{variable} must be set to a non-empty value (no mainnet fallback, #3064)")]
pub struct ChainBindingError {
    /// The configuration variable that must be supplied.
    pub variable: &'static str,
}

impl ChainBinding {
    /// The legacy mainnet binding — only ever selected explicitly.
    pub const MAINNET: Self = Self {
        chain_id: std::borrow::Cow::Borrowed("wrfcoin-main"),
        network_id: std::borrow::Cow::Borrowed("mainnet"),
    };

    /// The binding a process with NO declared identity signs and verifies
    /// under. It is not a chain: no node holds it (#2713 makes a node refuse to
    /// start without a declared identity), so a receipt signed under it is
    /// rejected by every node instead of being a well-formed mainnet receipt.
    /// A process without identity stays self-consistent (it can verify what it
    /// signed), which keeps in-process round trips honest without inventing a
    /// chain.
    pub const UNBOUND: Self = Self {
        chain_id: std::borrow::Cow::Borrowed("unbound"),
        network_id: std::borrow::Cow::Borrowed("unbound"),
    };

    /// Validate a caller-supplied chain and network; blank values are errors.
    ///
    /// Values are kept **verbatim**, never trimmed. That is exactly what
    /// `configured_chain_id()` / `declared_chain_identity()` in
    /// `wrfcoin_shared_protocol::crypto::domain_signing` and the pre-#3064
    /// resolver did: a whitespace-only value is refused, but a declared `"777 "`
    /// is folded in as `"777 "`. Trimming here would make a node with a padded
    /// identity verify receipts under a different message than an older binary
    /// on the same chain. Receipt verification runs in `ApplyReveal` and in the
    /// PoP reward scorers, so that would be a fork. It is pinned by
    /// `ambient_message_is_byte_identical_to_the_pre_3064_resolver_for_every_declared_identity`.
    pub fn new(
        chain_id: impl Into<String>,
        network_id: impl Into<String>,
    ) -> Result<Self, ChainBindingError> {
        let chain_id = chain_id.into();
        let network_id = network_id.into();
        for (variable, value) in [(CHAIN_ID_ENV, &chain_id), (NETWORK_ID_ENV, &network_id)] {
            if value.trim().is_empty() {
                return Err(ChainBindingError { variable });
            }
        }
        Ok(Self {
            chain_id: chain_id.into(),
            network_id: network_id.into(),
        })
    }

    /// Read both variables; unset, non-UTF-8 and blank values fail closed and
    /// name the variable. Mirrors `configured_chain_id()` in
    /// `wrfcoin_shared_protocol::crypto::domain_signing` (a dependent crate,
    /// so it cannot be imported here without a cycle).
    pub fn from_env() -> Result<Self, ChainBindingError> {
        let read = |variable| std::env::var(variable).map_err(|_| ChainBindingError { variable });
        Self::new(read(CHAIN_ID_ENV)?, read(NETWORK_ID_ENV)?)
    }

    /// Whether this is the sentinel a process without identity signs under.
    pub fn is_unbound(&self) -> bool {
        *self == Self::UNBOUND
    }

    /// Chain identifier folded into canonical messages.
    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }

    /// Network identifier folded into canonical messages.
    pub fn network_id(&self) -> &str {
        &self.network_id
    }
}

/// The binding the no-argument signing wrappers use: the declared identity,
/// or [`ChainBinding::UNBOUND`] — never mainnet by omission.
fn ambient_binding() -> ChainBinding {
    ChainBinding::from_env().unwrap_or(ChainBinding::UNBOUND)
}

// ---------------------------------------------------------------------------
// ForecastArtifactManifest
// ---------------------------------------------------------------------------

/// Canonical representation of forecast inputs and outputs for deterministic verification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastArtifactManifest {
    /// Schema version for the manifest layout.
    pub schema_version: u32,
    /// Forecast model identifier.
    pub model_id: String,
    /// Hex-encoded digest of the forecast inputs.
    pub input_hash: String,
    /// Hex-encoded digest of the forecast outputs.
    pub output_hash: String,
    /// Unix timestamp at which the forecast artifacts were produced.
    pub produced_at: i64,
    /// Geographic region covered by the forecast.
    pub region: String,
    /// Forecast horizon in hours.
    pub horizon_hours: u32,
    /// Sorted list of input variable names.
    pub input_variables: Vec<String>,
    /// Sorted list of output variable names.
    pub output_variables: Vec<String>,
}

/// Validation errors for forecast artifact manifests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForecastManifestValidationError {
    /// Schema version is not the supported value.
    #[error("SP_TX_MANIFEST_SCHEMA_VERSION: schema_version must be 1")]
    InvalidSchemaVersion,
    /// Model identifier is missing.
    #[error("SP_TX_MANIFEST_MODEL_ID_EMPTY: model_id must not be empty")]
    EmptyModelId,
    /// Model identifier exceeds the maximum length.
    #[error("SP_TX_MANIFEST_MODEL_ID_TOO_LONG: model_id exceeds 128 characters")]
    ModelIdTooLong,
    /// Input hash is not a 64-character hex string.
    #[error("SP_TX_MANIFEST_INPUT_HASH_FORMAT: input_hash must be a 64-character hex string")]
    InvalidInputHash,
    /// Output hash is not a 64-character hex string.
    #[error("SP_TX_MANIFEST_OUTPUT_HASH_FORMAT: output_hash must be a 64-character hex string")]
    InvalidOutputHash,
    /// Forecast horizon is outside the accepted range.
    #[error("SP_TX_MANIFEST_HORIZON_RANGE: horizon_hours must be between 1 and 720")]
    InvalidHorizonRange,
    /// Input variable list is empty.
    #[error("SP_TX_MANIFEST_INPUT_VARS_EMPTY: input_variables must not be empty")]
    EmptyInputVariables,
    /// Output variable list is empty.
    #[error("SP_TX_MANIFEST_OUTPUT_VARS_EMPTY: output_variables must not be empty")]
    EmptyOutputVariables,
    /// Input variable list is not sorted.
    #[error("SP_TX_MANIFEST_INPUT_VARS_UNSORTED: input_variables must be sorted")]
    InputVariablesUnsorted,
    /// Output variable list is not sorted.
    #[error("SP_TX_MANIFEST_OUTPUT_VARS_UNSORTED: output_variables must be sorted")]
    OutputVariablesUnsorted,
    /// Input variable list contains duplicates.
    #[error("SP_TX_MANIFEST_INPUT_VARS_DUPLICATES: input_variables contains duplicates")]
    InputVariablesDuplicates,
    /// Output variable list contains duplicates.
    #[error("SP_TX_MANIFEST_OUTPUT_VARS_DUPLICATES: output_variables contains duplicates")]
    OutputVariablesDuplicates,
}

impl ForecastArtifactManifest {
    /// Computes the deterministic digest of the manifest.
    pub fn manifest_digest(&self) -> String {
        payload_digest_hex(self)
    }

    /// Validates manifest fields according to protocol rules.
    pub fn validate(&self) -> Result<(), ForecastManifestValidationError> {
        if self.schema_version != 1 {
            return Err(ForecastManifestValidationError::InvalidSchemaVersion);
        }
        if self.model_id.is_empty() {
            return Err(ForecastManifestValidationError::EmptyModelId);
        }
        if self.model_id.len() > 128 {
            return Err(ForecastManifestValidationError::ModelIdTooLong);
        }
        if !is_valid_hex64(&self.input_hash) {
            return Err(ForecastManifestValidationError::InvalidInputHash);
        }
        if !is_valid_hex64(&self.output_hash) {
            return Err(ForecastManifestValidationError::InvalidOutputHash);
        }
        if !(1..=720).contains(&self.horizon_hours) {
            return Err(ForecastManifestValidationError::InvalidHorizonRange);
        }
        if self.input_variables.is_empty() {
            return Err(ForecastManifestValidationError::EmptyInputVariables);
        }
        if self.output_variables.is_empty() {
            return Err(ForecastManifestValidationError::EmptyOutputVariables);
        }
        if !is_sorted_and_unique(&self.input_variables) {
            return if has_duplicates(&self.input_variables) {
                Err(ForecastManifestValidationError::InputVariablesDuplicates)
            } else {
                Err(ForecastManifestValidationError::InputVariablesUnsorted)
            };
        }
        if !is_sorted_and_unique(&self.output_variables) {
            return if has_duplicates(&self.output_variables) {
                Err(ForecastManifestValidationError::OutputVariablesDuplicates)
            } else {
                Err(ForecastManifestValidationError::OutputVariablesUnsorted)
            };
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ForecastVerificationReceipt
// ---------------------------------------------------------------------------

/// Validation errors for forecast accuracy metrics (blockchain context).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForecastMetricsValidationError {
    /// MAE must be finite and non-negative.
    #[error("SP_TX_METRICS_MAE_INVALID: mae must be finite and non-negative")]
    InvalidMae,
    /// RMSE must be finite and non-negative.
    #[error("SP_TX_METRICS_RMSE_INVALID: rmse must be finite and non-negative")]
    InvalidRmse,
    /// Bias must be finite.
    #[error("SP_TX_METRICS_BIAS_INVALID: bias must be finite")]
    InvalidBias,
    /// Skill score must be in range.
    #[error("SP_TX_METRICS_SKILL_SCORE_RANGE: skill_score must be in [0, 1]")]
    InvalidSkillScore,
}

/// Validates [`ForecastAccuracyMetrics`] according to blockchain protocol rules.
///
/// This is a richer validation than the simple `&'static str` returned by
/// [`ForecastAccuracyMetrics::validate`], providing typed error variants that
/// can be embedded in [`ForecastReceiptValidationError::InvalidMetrics`].
pub fn validate_metrics(
    metrics: &ForecastAccuracyMetrics,
) -> Result<(), ForecastMetricsValidationError> {
    if !metrics.mae.is_finite() || metrics.mae < 0.0 {
        return Err(ForecastMetricsValidationError::InvalidMae);
    }
    if !metrics.rmse.is_finite() || metrics.rmse < 0.0 {
        return Err(ForecastMetricsValidationError::InvalidRmse);
    }
    if !metrics.bias.is_finite() {
        return Err(ForecastMetricsValidationError::InvalidBias);
    }
    if !metrics.skill_score.is_finite() || !(0.0..=1.0).contains(&metrics.skill_score) {
        return Err(ForecastMetricsValidationError::InvalidSkillScore);
    }
    Ok(())
}

/// Deterministic scoring receipt from forecast verification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastVerificationReceipt {
    /// Manifest digest that was verified.
    pub manifest_digest: String,
    /// Forecast commit identifier.
    pub commit_id: String,
    /// Oracle identifier that produced the receipt.
    pub oracle_id: String,
    /// Accuracy metrics recorded by verification.
    pub metrics: ForecastAccuracyMetrics,
    /// Verification timestamp as a Unix epoch in seconds.
    pub verified_at: i64,
    /// Anti-replay nonce.
    pub nonce: String,
    /// Optional hex-encoded ZK proof for model execution.
    ///
    /// # Wire compatibility: no `skip_serializing_if` (#2922)
    ///
    /// This receipt is decoded with **bincode**
    /// (`native-chain/src/bin/launch_api/p2p_events.rs` via
    /// `bincode::deserialize`), which is positional and non-self-describing.
    /// `skip_serializing_if` omits the field on the wire when `None`, but
    /// nothing on the decode side ever agreed to that: bincode's derived
    /// `Deserialize` has no read-side counterpart to `skip_serializing_if`
    /// (`skip_deserializing` is a different attribute and is not used here)
    /// — it always reads a fixed positional layout, unconditionally
    /// expecting an `Option` tag byte for this field. So a receipt encoded
    /// with `zk_proof: None` was undecodable by ANY receiver, old or new;
    /// this was never a version-skew hazard, it was broken from the first
    /// commit that added the attribute. Same defect class documented on
    /// `p2p::sync::BlockResponse::retry_after_ms` (not an intra-doc link:
    /// this crate does not depend on `p2p`), arriving here through the same
    /// attribute rather than a layout change.
    ///
    /// Dropping `skip_serializing_if` always writes the tag byte, matching
    /// what every existing decoder (this field has never NOT existed on the
    /// wire, unlike `BlockResponse::retry_after_ms`, which was a genuinely
    /// new field) has always required to read it. `Some`-encoded receipts
    /// are byte-identical before and after this change (`skip_serializing_if`
    /// only ever affected the `None` case). So unlike `BlockResponse`, this
    /// is NOT a breaking wire change requiring lockstep deployment: a fixed
    /// sender's `None` receipts newly decode correctly on an unpatched
    /// receiver (whose decoder already expected the tag byte), and an
    /// unpatched sender's `None` receipts remain broken until it, too, is
    /// patched — deploying the fix to one side only ever helps, never hurts.
    /// Pinned by `none_bearing_receipt_round_trips_through_bincode_2922`.
    #[serde(default)]
    pub zk_proof: Option<String>,
    /// Optional receipt signature. See `zk_proof`'s doc-comment above — the
    /// same #2922 wire-compatibility reasoning applies verbatim.
    #[serde(default)]
    pub signature: Option<String>,
}

/// Validation errors for forecast verification receipts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ForecastReceiptValidationError {
    /// Manifest digest is not a 64-character hex string.
    #[error(
        "SP_TX_RECEIPT_MANIFEST_DIGEST_FORMAT: manifest_digest must be a 64-character hex string"
    )]
    InvalidManifestDigest,
    /// Commit identifier is empty.
    #[error("SP_TX_RECEIPT_COMMIT_ID_EMPTY: commit_id must not be empty")]
    EmptyCommitId,
    /// Oracle identifier is empty.
    #[error("SP_TX_RECEIPT_ORACLE_ID_EMPTY: oracle_id must not be empty")]
    EmptyOracleId,
    /// Verification timestamp is invalid.
    #[error("SP_TX_RECEIPT_VERIFIED_AT_INVALID: verified_at must be positive")]
    InvalidVerifiedAt,
    /// Nested metrics payload is invalid.
    #[error("SP_TX_RECEIPT_METRICS_INVALID: {0}")]
    InvalidMetrics(ForecastMetricsValidationError),
}

impl ForecastVerificationReceipt {
    /// Computes the deterministic digest of the receipt.
    pub fn receipt_digest(&self) -> String {
        payload_digest_hex(self)
    }

    fn canonical_signing_message(&self, binding: &ChainBinding) -> String {
        #[derive(Serialize)]
        struct SigningPayload<'a> {
            manifest_digest: &'a str,
            commit_id: &'a str,
            oracle_id: &'a str,
            metrics: &'a ForecastAccuracyMetrics,
            verified_at: i64,
            nonce: &'a str,
        }

        let digest = payload_digest_hex(&SigningPayload {
            manifest_digest: &self.manifest_digest,
            commit_id: &self.commit_id,
            oracle_id: &self.oracle_id,
            metrics: &self.metrics,
            verified_at: self.verified_at,
            nonce: &self.nonce,
        });
        build_canonical_signing_message(
            binding,
            DomainSigningAction::ForecastVerification,
            &self.oracle_id,
            self.verified_at,
            Some(&self.nonce),
            &digest,
        )
    }

    /// Builds the canonical signing message under the process's declared
    /// identity (`WRFCOIN_CHAIN_ID` / `WRFCOIN_NETWORK_ID`). With no declared
    /// identity the message is built under [`ChainBinding::UNBOUND`], which
    /// verifies nowhere — never under mainnet by omission (#3064). Signers
    /// that want the error instead call [`Self::checked_signing_message`].
    pub fn signing_message(&self) -> String {
        self.signing_message_with(&ambient_binding())
    }

    /// The declared-identity signing message, or the configuration error.
    pub fn checked_signing_message(&self) -> Result<String, ChainBindingError> {
        Ok(self.signing_message_with(&ChainBinding::from_env()?))
    }

    /// Builds the canonical signing message under an explicit binding.
    pub fn signing_message_with(&self, binding: &ChainBinding) -> String {
        self.canonical_signing_message(binding)
    }

    /// Verifies the signature against the receipt `oracle_id` public key under
    /// the process's declared identity — or [`ChainBinding::UNBOUND`] when
    /// there is none, so a mainnet-signed receipt never verifies by omission
    /// (#3064). Use [`Self::has_valid_signature_with`] to name the chain.
    pub fn has_valid_signature(&self) -> bool {
        self.has_valid_signature_with(&ambient_binding())
    }

    /// Verifies the signature against `oracle_id` under an explicit binding.
    pub fn has_valid_signature_with(&self, binding: &ChainBinding) -> bool {
        let Some(signature_hex) = self.signature.as_deref() else {
            return false;
        };
        let Ok(public_key_bytes) = hex::decode(&self.oracle_id) else {
            return false;
        };
        let Ok(signature_bytes) = hex::decode(signature_hex) else {
            return false;
        };
        let Ok(public_key) = verifying_key_from_bytes(&public_key_bytes) else {
            return false;
        };
        let Ok(signature) = signature_from_bytes(&signature_bytes) else {
            return false;
        };
        public_key
            .verify(self.signing_message_with(binding).as_bytes(), &signature)
            .is_ok()
    }

    /// Validates receipt fields and nested metrics.
    pub fn validate(&self) -> Result<(), ForecastReceiptValidationError> {
        if !is_valid_hex64(&self.manifest_digest) {
            return Err(ForecastReceiptValidationError::InvalidManifestDigest);
        }
        if self.commit_id.is_empty() {
            return Err(ForecastReceiptValidationError::EmptyCommitId);
        }
        if self.oracle_id.is_empty() {
            return Err(ForecastReceiptValidationError::EmptyOracleId);
        }
        if self.verified_at <= 0 {
            return Err(ForecastReceiptValidationError::InvalidVerifiedAt);
        }
        validate_metrics(&self.metrics)
            .map_err(ForecastReceiptValidationError::InvalidMetrics)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Signing infrastructure
// ---------------------------------------------------------------------------

/// Domain-specific signing action identifiers.
enum DomainSigningAction {
    /// Forecast verification receipt signing.
    ForecastVerification,
}

impl DomainSigningAction {
    /// Returns the domain string embedded in signing messages.
    fn domain(&self) -> &'static str {
        match self {
            Self::ForecastVerification => "wrfcoin.forecast_verification.v1",
        }
    }
}

/// Errors encountered when decoding ed25519 keys or signatures from bytes.
enum SignatureError {
    /// Public key byte slice is not the correct length.
    PublicKeyLength,
    /// Public key bytes do not represent a valid ed25519 key.
    PublicKey,
    /// Signature byte slice is not the correct length.
    SignatureLength,
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Returns `true` if `value` is exactly 64 ASCII hex characters.
fn is_valid_hex64(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Returns `true` if `values` is strictly sorted with no duplicates.
fn is_sorted_and_unique(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

/// Returns `true` if `values` contains any duplicate entries.
fn has_duplicates(values: &[String]) -> bool {
    let mut seen = FxHashSet::default();
    values.iter().any(|value| !seen.insert(value))
}

/// Computes a deterministic SHA3-256 hex digest of the JSON-serialized payload.
fn payload_digest_hex<T: Serialize>(payload: &T) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(serde_json::to_vec(payload).unwrap_or_default());
    hex::encode(hasher.finalize())
}

/// Builds a canonical signing message from the given components.
fn build_canonical_signing_message(
    binding: &ChainBinding,
    action: DomainSigningAction,
    signer_id: &str,
    timestamp: i64,
    nonce: Option<&str>,
    payload_digest: &str,
) -> String {
    // The binding is explicit (#3064): sign and verify agree because both are
    // handed the same `ChainBinding`, not because both read the same
    // environment with the same fallback.
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        SIGNING_SCHEME_VERSION,
        action.domain(),
        binding.chain_id(),
        binding.network_id(),
        signer_id,
        timestamp,
        nonce.unwrap_or(""),
        payload_digest
    )
}

/// Decodes an ed25519 verifying (public) key from a byte slice.
fn verifying_key_from_bytes(bytes: &[u8]) -> Result<VerifyingKey, SignatureError> {
    let array: [u8; ed25519_dalek::PUBLIC_KEY_LENGTH] = bytes
        .try_into()
        .map_err(|_| SignatureError::PublicKeyLength)?;
    VerifyingKey::from_bytes(&array).map_err(|_| SignatureError::PublicKey)
}

/// Decodes an ed25519 signature from a byte slice.
fn signature_from_bytes(bytes: &[u8]) -> Result<Ed25519Signature, SignatureError> {
    if bytes.len() != ed25519_dalek::SIGNATURE_LENGTH {
        return Err(SignatureError::SignatureLength);
    }
    let array: [u8; ed25519_dalek::SIGNATURE_LENGTH] = bytes
        .try_into()
        .map_err(|_| SignatureError::SignatureLength)?;
    Ok(Ed25519Signature::from_bytes(&array))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    /// The signing environment is process-global: every test that touches it
    /// runs under this lock and restores what it found.
    fn with_identity<T>(chain: Option<&str>, network: Option<&str>, run: impl FnOnce() -> T) -> T {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev = (std::env::var(CHAIN_ID_ENV).ok(), std::env::var(NETWORK_ID_ENV).ok());
        let set = |key: &str, value: Option<&str>| unsafe {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        };
        set(CHAIN_ID_ENV, chain);
        set(NETWORK_ID_ENV, network);
        let out = run();
        set(CHAIN_ID_ENV, prev.0.as_deref());
        set(NETWORK_ID_ENV, prev.1.as_deref());
        out
    }

    fn signed_receipt(binding: &ChainBinding) -> ForecastVerificationReceipt {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let mut receipt = ForecastVerificationReceipt {
            manifest_digest: "c".repeat(64),
            commit_id: "commit-3064".to_string(),
            oracle_id: hex::encode(key.verifying_key().to_bytes()),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-3064".to_string(),
            zk_proof: None,
            signature: None,
        };
        let signature = key.sign(receipt.signing_message_with(binding).as_bytes());
        receipt.signature = Some(hex::encode(signature.to_bytes()));
        receipt
    }

    /// The pre-#3064 identity resolution, FROZEN verbatim from main
    /// `ab1b1a493` (`forecast-score/src/blockchain.rs` `signing_chain_id` /
    /// `signing_network_id` and the `build_canonical_signing_message` format).
    /// It deliberately does not call any #3064 code. Only the payload digest
    /// and the domain tag are shared, and #3064 does not touch them.
    fn frozen_pre_3064_signing_message(receipt: &ForecastVerificationReceipt) -> String {
        #[derive(Serialize)]
        struct SigningPayload<'a> {
            manifest_digest: &'a str,
            commit_id: &'a str,
            oracle_id: &'a str,
            metrics: &'a ForecastAccuracyMetrics,
            verified_at: i64,
            nonce: &'a str,
        }
        let chain_id = std::env::var("WRFCOIN_CHAIN_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "wrfcoin-main".to_string());
        let network_id = std::env::var("WRFCOIN_NETWORK_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "mainnet".to_string());
        let digest = payload_digest_hex(&SigningPayload {
            manifest_digest: &receipt.manifest_digest,
            commit_id: &receipt.commit_id,
            oracle_id: &receipt.oracle_id,
            metrics: &receipt.metrics,
            verified_at: receipt.verified_at,
            nonce: &receipt.nonce,
        });
        format!(
            // secret-leak-ok: test; a receipt SIGNING MESSAGE is a public preimage (chain/network ids, oracle id, nonce, digest), no key material
            "{}|{}|{}|{}|{}|{}|{}|{}",
            "wrfcoin-sign-v1",
            DomainSigningAction::ForecastVerification.domain(),
            chain_id,
            network_id,
            receipt.oracle_id,
            receipt.verified_at,
            receipt.nonce,
            digest
        )
    }

    /// Consensus pin (#3120 gating analysis): receipt signatures are verified
    /// in `ApplyReveal` (`verify_receipt_with_slashing`) and in both PoP reward
    /// scorers, under the node's env identity. A node cannot boot without a
    /// declared identity (#2713), so for EVERY declared identity, padded ones
    /// included, the ambient message must equal what the pre-#3064 binary
    /// built. Otherwise mixed-version nodes disagree on slashing and payout.
    #[test]
    fn ambient_message_is_byte_identical_to_the_pre_3064_resolver_for_every_declared_identity() {
        let receipt = signed_receipt(&ChainBinding::MAINNET);
        for (chain, network) in [
            ("777", "testnet"),
            ("777 ", "testnet"),
            (" 777", " testnet"),
            ("\t777\n", "testnet "),
            ("wrfcoin-main", "mainnet"),
            ("x", "y"),
        ] {
            with_identity(Some(chain), Some(network), || {
                let frozen = frozen_pre_3064_signing_message(&receipt);
                assert_eq!(
                    // secret-leak-ok: test; a receipt SIGNING MESSAGE is a public preimage (chain/network ids, oracle id, nonce, digest), no key material
                    receipt.signing_message(),
                    frozen,
                    "declared identity {chain:?}/{network:?} must sign exactly as before #3064"
                );
                assert_eq!(receipt.checked_signing_message().unwrap(), frozen); // secret-leak-ok: test; a receipt SIGNING MESSAGE is a public preimage (chain/network ids, oracle id, nonce, digest), no key material
                assert_eq!(
                    // secret-leak-ok: test; a receipt SIGNING MESSAGE is a public preimage (chain/network ids, oracle id, nonce, digest), no key material
                    receipt.signing_message_with(&ChainBinding::from_env().unwrap()),
                    frozen
                );
            });
        }
    }

    #[test]
    fn declared_identity_is_folded_in_and_equals_the_explicit_binding_3064() {
        with_identity(Some("777"), Some("testnet"), || {
            let binding = ChainBinding::from_env().unwrap();
            assert_eq!((binding.chain_id(), binding.network_id()), ("777", "testnet"));
            let receipt = signed_receipt(&binding);
            let message = receipt.signing_message();
            assert!(message.contains("|777|testnet|"), "{message}");
            assert_eq!(message, receipt.signing_message_with(&ChainBinding::new("777", "testnet").unwrap())); // secret-leak-ok: test; a receipt SIGNING MESSAGE is a public preimage (chain/network ids, oracle id, nonce, digest), no key material
            assert_eq!(receipt.checked_signing_message().unwrap(), message); // secret-leak-ok: test; a receipt SIGNING MESSAGE is a public preimage (chain/network ids, oracle id, nonce, digest), no key material
            assert!(receipt.has_valid_signature());
            assert!(receipt.has_valid_signature_with(&binding));
            assert!(!receipt.has_valid_signature_with(&ChainBinding::MAINNET), "bound to 777, not mainnet");
        });
    }

    #[test]
    fn a_missing_identity_fails_closed_and_never_means_mainnet_3064() {
        with_identity(None, None, || {
            let err = ChainBinding::from_env().unwrap_err();
            assert_eq!(err.variable, CHAIN_ID_ENV);
            assert!(err.to_string().contains("WRFCOIN_CHAIN_ID"), "{err}");
            let receipt = signed_receipt(&ChainBinding::MAINNET);
            assert!(receipt.checked_signing_message().is_err(), "a signer is told, not defaulted"); // secret-leak-ok: test; a receipt SIGNING MESSAGE is a public preimage (chain/network ids, oracle id, nonce, digest), no key material
            let ambient = receipt.signing_message();
            assert!(ambient.contains("|unbound|unbound|"), "{ambient}");
            assert_ne!(ambient, receipt.signing_message_with(&ChainBinding::MAINNET)); // secret-leak-ok: test; a receipt SIGNING MESSAGE is a public preimage (chain/network ids, oracle id, nonce, digest), no key material
            // The hazard this closes: a mainnet-signed receipt used to verify
            // here by omission. It no longer does; naming mainnet still works.
            assert!(!receipt.has_valid_signature());
            assert!(receipt.has_valid_signature_with(&ChainBinding::MAINNET));
        });
        with_identity(Some("777"), Some("   "), || {
            assert_eq!(ChainBinding::from_env().unwrap_err().variable, NETWORK_ID_ENV);
        });
        assert_eq!(ChainBinding::new("", "testnet").unwrap_err().variable, CHAIN_ID_ENV);
        assert!(ChainBinding::UNBOUND.is_unbound());
        assert!(!ChainBinding::MAINNET.is_unbound());
    }

    #[test]
    fn an_identity_less_process_stays_self_consistent_but_no_node_accepts_it_3064() {
        with_identity(None, None, || {
            let receipt = signed_receipt(&ChainBinding::UNBOUND);
            assert!(receipt.has_valid_signature(), "in-process round trip");
        });
        // A node always has a declared identity (#2713): under it the
        // unbound receipt is refused.
        with_identity(Some("777"), Some("testnet"), || {
            let receipt = signed_receipt(&ChainBinding::UNBOUND);
            assert!(!receipt.has_valid_signature());
        });
    }

    fn valid_manifest() -> ForecastArtifactManifest {
        ForecastArtifactManifest {
            schema_version: 1,
            model_id: "wrf-gfs-v3".to_string(),
            input_hash: "a".repeat(64),
            output_hash: "b".repeat(64),
            produced_at: 1_700_000_000,
            region: "us-east".to_string(),
            horizon_hours: 48,
            input_variables: vec!["humidity".to_string(), "pressure".to_string()],
            output_variables: vec!["precipitation".to_string(), "temperature".to_string()],
        }
    }

    fn valid_metrics() -> ForecastAccuracyMetrics {
        ForecastAccuracyMetrics {
            mae: 1.5,
            rmse: 2.0,
            bias: -0.3,
            skill_score: 0.85,
        }
    }

    #[test]
    fn manifest_validation_accepts_valid_input() {
        assert!(valid_manifest().validate().is_ok());
    }

    #[test]
    fn manifest_validation_rejects_bad_schema_version() {
        let mut m = valid_manifest();
        m.schema_version = 2;
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InvalidSchemaVersion)
        ));
    }

    #[test]
    fn manifest_validation_rejects_empty_model_id() {
        let mut m = valid_manifest();
        m.model_id = String::new();
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::EmptyModelId)
        ));
    }

    #[test]
    fn manifest_validation_rejects_long_model_id() {
        let mut m = valid_manifest();
        m.model_id = "x".repeat(129);
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::ModelIdTooLong)
        ));
    }

    #[test]
    fn manifest_validation_rejects_bad_input_hash() {
        let mut m = valid_manifest();
        m.input_hash = "not-hex".to_string();
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InvalidInputHash)
        ));
    }

    #[test]
    fn manifest_validation_rejects_bad_output_hash() {
        let mut m = valid_manifest();
        m.output_hash = "short".to_string();
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InvalidOutputHash)
        ));
    }

    #[test]
    fn manifest_validation_rejects_bad_horizon() {
        let mut m = valid_manifest();
        m.horizon_hours = 0;
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InvalidHorizonRange)
        ));
        m.horizon_hours = 721;
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InvalidHorizonRange)
        ));
    }

    #[test]
    fn manifest_validation_rejects_empty_vars() {
        let mut m = valid_manifest();
        m.input_variables = vec![];
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::EmptyInputVariables)
        ));

        let mut m2 = valid_manifest();
        m2.output_variables = vec![];
        assert!(matches!(
            m2.validate(),
            Err(ForecastManifestValidationError::EmptyOutputVariables)
        ));
    }

    #[test]
    fn manifest_validation_rejects_unsorted_vars() {
        let mut m = valid_manifest();
        m.input_variables = vec!["z".to_string(), "a".to_string()];
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InputVariablesUnsorted)
        ));
    }

    #[test]
    fn manifest_validation_rejects_duplicate_vars() {
        let mut m = valid_manifest();
        m.input_variables = vec!["a".to_string(), "a".to_string()];
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InputVariablesDuplicates)
        ));
    }

    #[test]
    fn manifest_digest_is_hex64() {
        let manifest = valid_manifest();
        let digest = manifest.manifest_digest();
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn metrics_validation_accepts_valid() {
        assert!(validate_metrics(&valid_metrics()).is_ok());
    }

    #[test]
    fn metrics_validation_rejects_invalid_mae() {
        let mut m = valid_metrics();
        m.mae = -1.0;
        assert!(matches!(
            validate_metrics(&m),
            Err(ForecastMetricsValidationError::InvalidMae)
        ));
    }

    #[test]
    fn metrics_validation_rejects_invalid_rmse() {
        let mut m = valid_metrics();
        m.rmse = f64::NAN;
        assert!(matches!(
            validate_metrics(&m),
            Err(ForecastMetricsValidationError::InvalidRmse)
        ));
    }

    #[test]
    fn metrics_validation_rejects_invalid_bias() {
        let mut m = valid_metrics();
        m.bias = f64::INFINITY;
        assert!(matches!(
            validate_metrics(&m),
            Err(ForecastMetricsValidationError::InvalidBias)
        ));
    }

    #[test]
    fn metrics_validation_rejects_invalid_skill_score() {
        let mut m = valid_metrics();
        m.skill_score = 1.5;
        assert!(matches!(
            validate_metrics(&m),
            Err(ForecastMetricsValidationError::InvalidSkillScore)
        ));
    }

    #[test]
    fn receipt_validation_rejects_invalid_metrics() {
        let mut receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: "oracle-1".to_string(),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };
        receipt.metrics.skill_score = 1.5;

        assert!(matches!(
            receipt.validate(),
            Err(ForecastReceiptValidationError::InvalidMetrics(
                ForecastMetricsValidationError::InvalidSkillScore
            ))
        ));
    }

    #[test]
    fn receipt_validation_rejects_bad_manifest_digest() {
        let receipt = ForecastVerificationReceipt {
            manifest_digest: "too-short".to_string(),
            commit_id: "commit-1".to_string(),
            oracle_id: "oracle-1".to_string(),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };
        assert!(matches!(
            receipt.validate(),
            Err(ForecastReceiptValidationError::InvalidManifestDigest)
        ));
    }

    #[test]
    fn receipt_validation_rejects_empty_commit_id() {
        let receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: String::new(),
            oracle_id: "oracle-1".to_string(),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };
        assert!(matches!(
            receipt.validate(),
            Err(ForecastReceiptValidationError::EmptyCommitId)
        ));
    }

    #[test]
    fn receipt_validation_rejects_empty_oracle_id() {
        let receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: String::new(),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };
        assert!(matches!(
            receipt.validate(),
            Err(ForecastReceiptValidationError::EmptyOracleId)
        ));
    }

    #[test]
    fn receipt_validation_rejects_invalid_verified_at() {
        let receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: "oracle-1".to_string(),
            metrics: valid_metrics(),
            verified_at: 0,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };
        assert!(matches!(
            receipt.validate(),
            Err(ForecastReceiptValidationError::InvalidVerifiedAt)
        ));
    }

    #[test]
    fn receipt_deserializes_when_optional_fields_are_missing() {
        let receipt: ForecastVerificationReceipt = serde_json::from_value(json!({
            "manifest_digest": "a".repeat(64),
            "commit_id": "commit-1",
            "oracle_id": "oracle-1",
            "metrics": {
                "mae": 1.5,
                "rmse": 2.0,
                "bias": -0.3,
                "skill_score": 0.85
            },
            "verified_at": 1_700_000_100i64,
            "nonce": "nonce-1"
        }))
        .expect("missing optional fields should deserialize with defaults");

        assert_eq!(receipt.zk_proof, None);
        assert_eq!(receipt.signature, None);
    }

    /// #2922: bincode is positional and non-self-describing. `skip_serializing_if`
    /// omits the field on the WIRE entirely when `None`, but nothing on the
    /// decode side ever agreed to that — the derived `Deserialize` always
    /// reads a fixed positional layout and unconditionally expects an
    /// `Option` tag byte for every field, `skip_serializing_if` or not (that
    /// attribute has no read-side counterpart; `skip_deserializing` is a
    /// different attribute and is not used here). So a `None`-bearing receipt
    /// currently produces a byte stream shorter than any decoder — old or
    /// new — has ever required, and `bincode::deserialize` fails.
    ///
    /// This is exactly the #2727-family hazard [`p2p::sync::BlockResponse`]'s
    /// doc-comment warns about (a `#[serde(default, skip_serializing_if =
    /// ...)]` field is additive-safe under JSON and actively wrong under
    /// bincode), arriving here through the same attribute on a type that
    /// already crosses a real peer boundary
    /// (`native-chain/src/bin/launch_api/p2p_events.rs` decodes
    /// `ForecastTaskResult`/`ForecastTaskResultV2` with
    /// `bincode::deserialize`).
    ///
    /// Both fields are optional BY DESIGN, so a `None`-bearing receipt is not
    /// a rare shape — it is the default for any receipt produced without a
    /// ZK proof or a signature. RED on current main; must go GREEN once
    /// `skip_serializing_if` is removed (see the field doc-comments above).
    #[test]
    fn none_bearing_receipt_round_trips_through_bincode_2922() {
        let receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: "oracle-1".to_string(),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };

        let wire = bincode::serialize(&receipt).expect("encoding a None-bearing receipt");

        let decoded: Result<ForecastVerificationReceipt, _> = bincode::deserialize(&wire);
        let decoded = decoded.unwrap_or_else(|e| {
            panic!(
                "a None-bearing ForecastVerificationReceipt must decode on the wire \
                 it is actually sent over -- one node producing a receipt another \
                 node cannot read is the exact #2727-family hazard this type must \
                 not repeat. bincode::deserialize failed: {e}"
            )
        });

        assert_eq!(decoded, receipt);
    }

    #[test]
    fn receipt_digest_is_hex64() {
        let receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: "oracle-1".to_string(),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };
        let digest = receipt.receipt_digest();
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn receipt_signature_round_trip() {
        let signing_key = SigningKey::from_bytes(&[7u8; ed25519_dalek::SECRET_KEY_LENGTH]);
        let verifying_key = signing_key.verifying_key();

        let mut receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: hex::encode(verifying_key.to_bytes()),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };

        // signing_message() and has_valid_signature() both read the process-global
        // chain identity; hold the env lock so a concurrent with_identity() test
        // cannot change it between signing and verifying (#3318).
        with_identity(Some("777"), Some("testnet"), || {
            let signature = signing_key.sign(receipt.signing_message().as_bytes());
            receipt.signature = Some(hex::encode(signature.to_bytes()));

            assert_eq!(receipt.receipt_digest().len(), 64);
            assert!(receipt.has_valid_signature());
        });
    }

    #[test]
    fn has_valid_signature_returns_false_without_signature() {
        let receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: "oracle-1".to_string(),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };
        assert!(!receipt.has_valid_signature());
    }

    #[test]
    fn has_valid_signature_returns_false_for_bad_oracle_id() {
        let receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: "not-hex".to_string(),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: Some("ab".repeat(64)),
        };
        assert!(!receipt.has_valid_signature());
    }
}
