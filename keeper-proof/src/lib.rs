//! Keeper storage attestation, retrieval proof, and health monitoring types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use wrf_primitives::Timestamp;
use wrfcoin_dataset_manifest::DatasetManifest;
use wrfcoin_shared_protocol::crypto::domain_signing::{
    DomainSeparated, DomainSigningAction, DomainSigningContext, canonical_signing_message,
    payload_digest_hex, signing_chain_id, signing_network_id,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};

/// Backward-compatible alias for the narrow keeper proof payload.
pub type KeeperProof = StorageAttestation;

/// Errors returned by keeper-proof helpers.
#[derive(Debug, Error)]
pub enum KeeperProofError {
    /// Failed to serialize a signable payload.
    #[error("failed to serialize keeper payload: {0}")]
    Serialize(#[from] serde_json::Error),
    /// Attestation timestamps cannot be represented in the signing context.
    #[error("attestation timestamp is out of range for signing context")]
    InvalidTimestamp,
    /// Signature bytes are malformed.
    #[error("invalid signature bytes")]
    InvalidSignature,
    /// Input validation failed.
    #[error("{0}")]
    Invalid(String),
}

/// Signed proof that a keeper holds a dataset CID for a bounded period.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StorageAttestation {
    /// Keeper identifier. This is a `String` until `OracleId` is extracted to primitives.
    pub keeper_id: String,
    /// Dataset content identifier being held.
    pub dataset_cid: String,
    /// Optional manifest CID derived from a dataset manifest.
    pub manifest_cid: Option<String>,
    /// Time when custody began.
    pub held_since: Timestamp,
    /// Time after which the attestation is stale.
    pub expires_at: Timestamp,
    /// Signature over the canonical signing payload.
    pub signature: Vec<u8>,
    /// Optional replay-protection nonce.
    #[serde(default)]
    pub nonce: Option<String>,
}

// #2249: `DomainSeparated` for the cross-crate signed type (tag reserved in
// `wrfcoin_shared_protocol::crypto::domain_registry`). Excludes `signature`;
// field ordering mirrors `SignableStorageAttestation`.
impl DomainSeparated for StorageAttestation {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.storage_attestation.v1";

    fn canonical_body(&self) -> Vec<u8> {
        wrfcoin_shared_protocol::crypto::domain_impls::BodyBuilder::new()
            .str_field(&self.keeper_id)
            .str_field(&self.dataset_cid)
            .opt_str_field(self.manifest_cid.as_deref())
            .u64_field(self.held_since.into_inner())
            .u64_field(self.expires_at.into_inner())
            .opt_str_field(self.nonce.as_deref())
            .finish()
    }
}

/// Proof that a keeper successfully served a retrieval request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalProof {
    /// Keeper that served the retrieval.
    pub keeper_id: String,
    /// Dataset content identifier that was retrieved.
    pub dataset_cid: String,
    /// Retrieval completion time.
    pub retrieved_at: Timestamp,
    /// End-to-end retrieval latency in milliseconds.
    pub latency_ms: u64,
    /// Whether the retrieval succeeded.
    pub success: bool,
}

/// Keeper service metadata used by placement and routing logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeeperRecord {
    /// Keeper identifier. This is a `String` until `OracleId` is extracted to primitives.
    pub keeper_id: String,
    /// Capacity advertised by the keeper in bytes.
    pub capacity_bytes: u64,
    /// Regions currently served by the keeper.
    pub regions_served: Vec<String>,
    /// Uptime ratio in the inclusive range `[0.0, 1.0]`.
    pub uptime_ratio: f64,
    /// Latest keeper heartbeat time.
    pub last_heartbeat: Timestamp,
}

/// Storage pin proof for provider-backed archival retention.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PinProof {
    /// Provider name such as IPFS or Filecoin.
    pub provider: String,
    /// Dataset content identifier that is pinned.
    pub dataset_cid: String,
    /// Time at which the provider asserted the pin.
    pub pinned_at: Timestamp,
    /// Optional expiry for the provider proof.
    pub expires_at: Option<Timestamp>,
    /// Provider-specific proof material.
    pub provider_proof: String,
}

/// Health state used by the in-memory monitor.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HealthStatus {
    /// No recent failures are recorded.
    Healthy,
    /// The backend has recent failures but can still be retried.
    Degraded,
    /// The backend is temporarily unavailable to routing due to the circuit breaker.
    OpenCircuit,
    /// Reserved explicit unhealthy state for callers that want a terminal label.
    Unhealthy,
}

/// Minimal circuit-breaker state for a single backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackendHealth {
    /// Consecutive failure count since the last success.
    pub consecutive_failures: u32,
    /// Latest successful request timestamp.
    pub last_success_at: Option<Timestamp>,
    /// Latest failed request timestamp.
    pub last_failure_at: Option<Timestamp>,
    /// When the open circuit should close again.
    pub circuit_open_until: Option<Timestamp>,
}

impl Default for BackendHealth {
    fn default() -> Self {
        Self {
            consecutive_failures: 0,
            last_success_at: None,
            last_failure_at: None,
            circuit_open_until: None,
        }
    }
}

/// Simple in-memory health monitor with circuit-breaker behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthMonitor {
    /// Consecutive failures before the circuit opens.
    pub failure_threshold: u32,
    /// Cooldown window in seconds once the circuit opens.
    pub cooldown_secs: u64,
    /// Backend state map keyed by backend identifier.
    pub backends: HashMap<String, BackendHealth>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignableStorageAttestation<'a> {
    keeper_id: &'a str,
    dataset_cid: &'a str,
    manifest_cid: Option<&'a str>,
    held_since: Timestamp,
    expires_at: Timestamp,
    nonce: Option<&'a str>,
}

impl StorageAttestation {
    /// Create and sign a storage attestation.
    pub fn sign(
        key: &PrivateKey,
        keeper_id: impl Into<String>,
        dataset_cid: impl Into<String>,
        held_since: Timestamp,
        expires_at: Timestamp,
        manifest_cid: Option<String>,
        nonce: Option<String>,
    ) -> Result<Self, KeeperProofError> {
        let unsigned = Self {
            keeper_id: keeper_id.into(),
            dataset_cid: dataset_cid.into(),
            manifest_cid,
            held_since,
            expires_at,
            signature: Vec::new(),
            nonce,
        };
        unsigned.validate()?;
        let signature = key.sign(&unsigned.signing_bytes()?).as_bytes().to_vec();

        Ok(Self {
            signature,
            ..unsigned
        })
    }

    /// Create and sign an attestation directly from a dataset manifest.
    pub fn from_manifest(
        key: &PrivateKey,
        keeper_id: impl Into<String>,
        manifest: &DatasetManifest,
        held_since: Timestamp,
        expires_at: Timestamp,
        nonce: Option<String>,
    ) -> Result<Self, KeeperProofError> {
        Self::sign(
            key,
            keeper_id,
            manifest.cid.clone(),
            held_since,
            expires_at,
            Some(manifest.manifest_cid()),
            nonce,
        )
    }

    /// Build the canonical domain-separated signing message.
    pub fn canonical_signing_message(&self) -> Result<String, KeeperProofError> {
        self.validate()?;
        let payload = SignableStorageAttestation {
            keeper_id: &self.keeper_id,
            dataset_cid: &self.dataset_cid,
            manifest_cid: self.manifest_cid.as_deref(),
            held_since: self.held_since,
            expires_at: self.expires_at,
            nonce: self.nonce.as_deref(),
        };
        let digest = payload_digest_hex(&payload);
        let cid = signing_chain_id();
        let nid = signing_network_id();
        let context = DomainSigningContext {
            action: DomainSigningAction::OracleAttestation,
            signer_id: &self.keeper_id,
            timestamp: i64::try_from(self.held_since.into_inner())
                .map_err(|_| KeeperProofError::InvalidTimestamp)?,
            nonce: self.nonce.as_deref(),
            chain_id: &cid,
            network_id: &nid,
        };
        Ok(canonical_signing_message(&context, &digest))
    }

    /// Verify the storage attestation signature.
    pub fn verify(&self, public_key: &PublicKey) -> bool {
        self.verify_checked(public_key).unwrap_or(false)
    }

    /// Verify the storage attestation signature, returning structural errors.
    pub fn verify_checked(&self, public_key: &PublicKey) -> Result<bool, KeeperProofError> {
        let signature = Signature::from_bytes(&self.signature)
            .map_err(|_| KeeperProofError::InvalidSignature)?;
        Ok(public_key.verify(&self.signing_bytes()?, &signature))
    }

    /// Returns `true` when the attestation should no longer be accepted.
    pub fn is_expired(&self, current_time: Timestamp) -> bool {
        current_time >= self.expires_at
    }

    /// Validate attestation fields without checking a signature.
    pub fn validate(&self) -> Result<(), KeeperProofError> {
        validate_trimmed("keeper_id", &self.keeper_id)?;
        validate_trimmed("dataset_cid", &self.dataset_cid)?;
        if let Some(manifest_cid) = self.manifest_cid.as_deref() {
            validate_trimmed("manifest_cid", manifest_cid)?;
        }
        if self.expires_at < self.held_since {
            return Err(KeeperProofError::Invalid(
                "expires_at must be greater than or equal to held_since".to_string(),
            ));
        }
        Ok(())
    }

    fn signing_bytes(&self) -> Result<Vec<u8>, KeeperProofError> {
        Ok(self.canonical_signing_message()?.into_bytes())
    }
}

impl KeeperRecord {
    /// Validate keeper metadata for basic structural correctness.
    pub fn validate(&self) -> Result<(), KeeperProofError> {
        validate_trimmed("keeper_id", &self.keeper_id)?;
        if !self.uptime_ratio.is_finite() || !(0.0..=1.0).contains(&self.uptime_ratio) {
            return Err(KeeperProofError::Invalid(
                "uptime_ratio must be finite and within [0.0, 1.0]".to_string(),
            ));
        }
        for region in &self.regions_served {
            validate_trimmed("regions_served", region)?;
        }
        Ok(())
    }
}

impl PinProof {
    /// Returns `true` when the provider proof is expired.
    pub fn is_expired(&self, current_time: Timestamp) -> bool {
        self.expires_at
            .map(|expires_at| current_time >= expires_at)
            .unwrap_or(false)
    }
}

impl HealthMonitor {
    /// Create a new health monitor.
    pub fn new(failure_threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            failure_threshold: failure_threshold.max(1),
            cooldown_secs,
            backends: HashMap::new(),
        }
    }

    /// Get the state for a backend, if any.
    pub fn backend(&self, backend_id: &str) -> Option<&BackendHealth> {
        self.backends.get(backend_id)
    }

    /// Record a successful backend interaction.
    pub fn record_success(&mut self, backend_id: impl Into<String>, current_time: Timestamp) {
        let state = self.backends.entry(backend_id.into()).or_default();
        state.consecutive_failures = 0;
        state.last_success_at = Some(current_time);
        state.circuit_open_until = None;
    }

    /// Record a failed backend interaction.
    pub fn record_failure(&mut self, backend_id: impl Into<String>, current_time: Timestamp) {
        let state = self.backends.entry(backend_id.into()).or_default();
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        state.last_failure_at = Some(current_time);
        if state.consecutive_failures >= self.failure_threshold {
            state.circuit_open_until = Some(Timestamp::new(
                current_time.into_inner().saturating_add(self.cooldown_secs),
            ));
        }
    }

    /// Determine the current routing status for a backend.
    pub fn status(&self, backend_id: &str, current_time: Timestamp) -> HealthStatus {
        let Some(state) = self.backends.get(backend_id) else {
            return HealthStatus::Healthy;
        };

        if let Some(circuit_open_until) = state.circuit_open_until {
            if current_time < circuit_open_until {
                return HealthStatus::OpenCircuit;
            }
        }

        if state.consecutive_failures == 0 {
            HealthStatus::Healthy
        } else if state.last_success_at.is_none() && state.last_failure_at.is_some() {
            HealthStatus::Unhealthy
        } else {
            HealthStatus::Degraded
        }
    }

    /// Select the first backend that is not currently circuit-open.
    pub fn select_backend<I, S>(&self, backends: I, current_time: Timestamp) -> Option<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        backends
            .into_iter()
            .map(|backend| backend.as_ref().to_string())
            .find(|backend| self.status(backend, current_time) != HealthStatus::OpenCircuit)
    }
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new(3, 300)
    }
}

fn validate_trimmed(field: &str, value: &str) -> Result<(), KeeperProofError> {
    if value.trim().is_empty() {
        return Err(KeeperProofError::Invalid(format!("{field} is required")));
    }
    if value.trim() != value {
        return Err(KeeperProofError::Invalid(format!(
            "{field} cannot contain leading or trailing whitespace"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;
    use wrfcoin_shared_protocol::crypto::signature::PublicKey;

    fn sample_manifest() -> DatasetManifest {
        DatasetManifest::new(
            "dataset-1".to_string(),
            "ipfs://bafydata".to_string(),
            "v1".to_string(),
            42,
            4096,
            "zstd".to_string(),
            "deadbeef".to_string(),
            1_730_000_000,
            "station-7".to_string(),
            "oracle-9".to_string(),
            None,
        )
    }

    #[test]
    fn storage_attestation_sign_verify_round_trip() {
        let mut rng = OsRng;
        let key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&key);
        let attestation = StorageAttestation::sign(
            &key,
            "keeper-1",
            "ipfs://bafydata",
            Timestamp::new(1_730_000_000),
            Timestamp::new(1_730_000_600),
            Some("sha256:manifest".to_string()),
            Some("nonce-1".to_string()),
        )
        .expect("signing should succeed");

        assert!(attestation.verify(&public_key));
    }

    /// #2249 sensitivity: a signed field is bound into the `DomainSeparated`
    /// body and the signature is excluded. Removing `.str_field(&self.keeper_id)`
    /// from `canonical_body` makes the first assertion RED; folding the signature
    /// in makes the second RED.
    #[test]
    fn storage_attestation_domain_body_sensitivity_and_signature_exclusion_2249() {
        use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;
        let mut rng = OsRng;
        let key = PrivateKey::generate(&mut rng);
        let signed = StorageAttestation::sign(
            &key,
            "keeper-1",
            "ipfs://bafydata",
            Timestamp::new(1_730_000_000),
            Timestamp::new(1_730_000_600),
            Some("sha256:manifest".to_string()),
            Some("nonce-1".to_string()),
        )
        .expect("signing should succeed");
        let base = signed.signing_preimage();

        let mut sig_only = signed.clone();
        sig_only.signature = vec![0xAB; 64];
        assert_eq!(
            base,
            sig_only.signing_preimage(),
            "signature must be excluded from the domain body"
        );

        let mut changed = signed.clone();
        changed.keeper_id = "keeper-2".to_string();
        assert_ne!(
            base,
            changed.signing_preimage(),
            "a signed field (keeper_id) must be bound into the domain body"
        );
    }

    #[test]
    fn storage_attestation_can_be_built_from_manifest() {
        let mut rng = OsRng;
        let key = PrivateKey::generate(&mut rng);
        let attestation = StorageAttestation::from_manifest(
            &key,
            "keeper-1",
            &sample_manifest(),
            Timestamp::new(1_730_000_000),
            Timestamp::new(1_730_000_600),
            None,
        )
        .expect("manifest-based signing should succeed");

        assert_eq!(attestation.dataset_cid, "ipfs://bafydata");
        assert!(attestation.manifest_cid.is_some());
    }

    #[test]
    fn storage_attestation_expiry_check() {
        let attestation = StorageAttestation {
            keeper_id: "keeper-1".to_string(),
            dataset_cid: "cid".to_string(),
            manifest_cid: None,
            held_since: Timestamp::new(10),
            expires_at: Timestamp::new(20),
            signature: vec![1; 64],
            nonce: None,
        };

        assert!(!attestation.is_expired(Timestamp::new(19)));
        assert!(attestation.is_expired(Timestamp::new(20)));
    }

    #[test]
    fn storage_attestation_tampered_signature_is_rejected() {
        let mut rng = OsRng;
        let key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&key);
        let mut attestation = StorageAttestation::sign(
            &key,
            "keeper-1",
            "cid",
            Timestamp::new(10),
            Timestamp::new(20),
            None,
            None,
        )
        .expect("signing should succeed");

        attestation.signature[0] ^= 0xFF;
        assert!(!attestation.verify(&public_key));
    }

    #[test]
    fn storage_attestation_tampered_payload_is_rejected() {
        let mut rng = OsRng;
        let key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&key);
        let mut attestation = StorageAttestation::sign(
            &key,
            "keeper-1",
            "cid",
            Timestamp::new(10),
            Timestamp::new(20),
            None,
            None,
        )
        .expect("signing should succeed");

        attestation.dataset_cid = "cid-2".to_string();
        assert!(!attestation.verify(&public_key));
    }

    #[test]
    fn keeper_record_validation_rejects_invalid_uptime() {
        let record = KeeperRecord {
            keeper_id: "keeper-1".to_string(),
            capacity_bytes: 1000,
            regions_served: vec!["us-west".to_string()],
            uptime_ratio: 1.5,
            last_heartbeat: Timestamp::new(10),
        };

        assert!(record.validate().is_err());
    }

    #[test]
    fn pin_proof_expiry_handles_optional_expiration() {
        let with_expiry = PinProof {
            provider: "ipfs".to_string(),
            dataset_cid: "cid".to_string(),
            pinned_at: Timestamp::new(10),
            expires_at: Some(Timestamp::new(20)),
            provider_proof: "proof".to_string(),
        };
        let without_expiry = PinProof {
            provider: "ipfs".to_string(),
            dataset_cid: "cid".to_string(),
            pinned_at: Timestamp::new(10),
            expires_at: None,
            provider_proof: "proof".to_string(),
        };

        assert!(with_expiry.is_expired(Timestamp::new(20)));
        assert!(!without_expiry.is_expired(Timestamp::new(999)));
    }

    #[test]
    fn health_monitor_opens_circuit_and_recovers_after_cooldown() {
        let mut monitor = HealthMonitor::new(2, 30);
        monitor.record_failure("ipfs-a", Timestamp::new(100));
        assert_eq!(
            monitor.status("ipfs-a", Timestamp::new(100)),
            HealthStatus::Unhealthy
        );
        monitor.record_failure("ipfs-a", Timestamp::new(105));
        assert_eq!(
            monitor.status("ipfs-a", Timestamp::new(110)),
            HealthStatus::OpenCircuit
        );
        assert_eq!(
            monitor.select_backend(["ipfs-a"], Timestamp::new(110)),
            None
        );
        assert_eq!(
            monitor.select_backend(["ipfs-a"], Timestamp::new(136)),
            Some("ipfs-a".to_string())
        );
        monitor.record_success("ipfs-a", Timestamp::new(137));
        assert_eq!(
            monitor.status("ipfs-a", Timestamp::new(138)),
            HealthStatus::Healthy
        );
    }
}
