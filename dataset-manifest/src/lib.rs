//! Canonical dataset manifest and provenance types for WRFCoin.
//!
//! This crate provides content-addressed dataset manifests, provenance chains,
//! and policy-driven provenance validation.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Signed provenance step binding an artifact hash to its parent lineage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceLink {
    pub artifact_hash: String,
    pub parent_hash: Option<String>,
    pub signer: String,
    pub signature: String,
    pub algorithm: String,
    pub signed_at: u64,
}

/// Provenance chain attached to incoming artifacts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceChain {
    pub chain_id: String,
    pub root_hash: String,
    pub links: Vec<ProvenanceLink>,
}

/// Result of provenance verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenanceValidationResult {
    pub valid: bool,
    pub reason: String,
    pub invalid_links: usize,
}

/// Routing decision used by ingestion logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AdmissionDecision {
    Accepted,
    Quarantined,
    Rejected,
}

/// Policy that governs what provenance chains are considered acceptable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvenancePolicy {
    pub require_provenance: bool,
    pub min_chain_length: usize,
    pub allowed_algorithms: Vec<String>,
}

impl Default for ProvenancePolicy {
    fn default() -> Self {
        Self {
            require_provenance: true,
            min_chain_length: 1,
            allowed_algorithms: Vec::new(),
        }
    }
}

/// Stateless provenance validator that performs structural and policy checks.
#[derive(Debug, Clone)]
pub struct ProvenanceValidator {
    policy: ProvenancePolicy,
}

impl ProvenanceValidator {
    pub fn new(policy: ProvenancePolicy) -> Self {
        Self { policy }
    }

    pub fn validate_chain(&self, chain: &ProvenanceChain) -> ProvenanceValidationResult {
        if chain.chain_id.trim().is_empty() || chain.root_hash.trim().is_empty() {
            return ProvenanceValidationResult {
                valid: false,
                reason: "chain_id and root_hash are required".to_string(),
                invalid_links: chain.links.len(),
            };
        }

        if chain.links.is_empty() {
            return ProvenanceValidationResult {
                valid: false,
                reason: "at least one provenance link is required".to_string(),
                invalid_links: 1,
            };
        }

        let mut invalid_links = 0usize;
        for (idx, link) in chain.links.iter().enumerate() {
            if link.artifact_hash.trim().is_empty()
                || link.signer.trim().is_empty()
                || link.signature.trim().is_empty()
                || link.algorithm.trim().is_empty()
            {
                invalid_links += 1;
                continue;
            }

            if idx == 0 {
                if link.artifact_hash != chain.root_hash {
                    invalid_links += 1;
                }
            } else {
                let prev = &chain.links[idx - 1];
                if link.parent_hash.as_deref() != Some(prev.artifact_hash.as_str()) {
                    invalid_links += 1;
                }
            }
        }

        if invalid_links > 0 {
            return ProvenanceValidationResult {
                valid: false,
                reason: "provenance chain continuity/signature metadata failed validation"
                    .to_string(),
                invalid_links,
            };
        }

        ProvenanceValidationResult {
            valid: true,
            reason: "valid provenance chain".to_string(),
            invalid_links: 0,
        }
    }

    pub fn validate_with_policy(&self, chain: &ProvenanceChain) -> Result<(), String> {
        let structural = self.validate_chain(chain);
        if !structural.valid {
            return Err(structural.reason);
        }

        if chain.links.len() < self.policy.min_chain_length {
            return Err(format!(
                "provenance chain has {} links but policy requires at least {}",
                chain.links.len(),
                self.policy.min_chain_length
            ));
        }

        if !self.policy.allowed_algorithms.is_empty() {
            for link in &chain.links {
                if !self.policy.allowed_algorithms.contains(&link.algorithm) {
                    return Err(format!(
                        "algorithm '{}' is not in the allowed algorithms list",
                        link.algorithm
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn decide_admission(
        &self,
        chain: Option<&ProvenanceChain>,
    ) -> (AdmissionDecision, Option<String>) {
        let chain = match chain {
            Some(c) => c,
            None => {
                if self.policy.require_provenance {
                    return (AdmissionDecision::Rejected, None);
                }
                return (AdmissionDecision::Accepted, None);
            },
        };

        match self.validate_with_policy(chain) {
            Ok(()) => (AdmissionDecision::Accepted, None),
            Err(reason) => (AdmissionDecision::Quarantined, Some(reason)),
        }
    }
}

/// Provider-specific archival receipt entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveProviderReceipt {
    pub provider: String,
    pub content_id: String,
    pub timestamp: u64,
    pub size: u64,
}

/// Receipt from archiving data to multiple providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveReceipt {
    pub canonical_cid: String,
    pub provider_receipts: Vec<ArchiveProviderReceipt>,
    pub archived_at: u64,
    pub providers_attempted: usize,
    pub providers_succeeded: usize,
    pub provenance_result: Option<ProvenanceValidationResult>,
    pub admission_decision: Option<AdmissionDecision>,
}

/// Top-level manifest for content-addressed datasets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetManifest {
    pub dataset_id: String,
    pub cid: String,
    pub schema_version: String,
    pub record_count: u64,
    pub byte_size: u64,
    pub compression: String,
    pub checksum: String,
    pub created_at: u64,
    pub station_id: String,
    pub oracle_id: String,
    pub provenance: Option<ProvenanceChain>,
}

impl DatasetManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dataset_id: String,
        cid: String,
        schema_version: String,
        record_count: u64,
        byte_size: u64,
        compression: String,
        checksum: String,
        created_at: u64,
        station_id: String,
        oracle_id: String,
        provenance: Option<ProvenanceChain>,
    ) -> Self {
        Self {
            dataset_id,
            cid,
            schema_version,
            record_count,
            byte_size,
            compression,
            checksum,
            created_at,
            station_id,
            oracle_id,
            provenance,
        }
    }

    pub fn manifest_cid(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("manifest serialization should not fail");
        let digest = Sha256::digest(bytes);
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        format!("sha256:{hex}")
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.dataset_id.trim().is_empty() {
            return Err("dataset_id is required".to_string());
        }
        if self.cid.trim().is_empty() {
            return Err("cid is required".to_string());
        }
        if self.schema_version.trim().is_empty() {
            return Err("schema_version is required".to_string());
        }
        if self.compression.trim().is_empty() {
            return Err("compression is required".to_string());
        }
        if self.checksum.trim().is_empty() {
            return Err("checksum is required".to_string());
        }
        if self.station_id.trim().is_empty() {
            return Err("station_id is required".to_string());
        }
        if self.oracle_id.trim().is_empty() {
            return Err("oracle_id is required".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_chain() -> ProvenanceChain {
        ProvenanceChain {
            chain_id: "chain-1".to_string(),
            root_hash: "root-hash".to_string(),
            links: vec![
                ProvenanceLink {
                    artifact_hash: "root-hash".to_string(),
                    parent_hash: None,
                    signer: "oracle-1".to_string(),
                    signature: "sig-1".to_string(),
                    algorithm: "ed25519".to_string(),
                    signed_at: 1_700_000_001,
                },
                ProvenanceLink {
                    artifact_hash: "child-hash".to_string(),
                    parent_hash: Some("root-hash".to_string()),
                    signer: "oracle-2".to_string(),
                    signature: "sig-2".to_string(),
                    algorithm: "ed25519".to_string(),
                    signed_at: 1_700_000_002,
                },
            ],
        }
    }

    fn valid_manifest() -> DatasetManifest {
        DatasetManifest::new(
            "dataset-123".to_string(),
            "bafybeigdyrzt4xexample".to_string(),
            "1.0.0".to_string(),
            10_000,
            12_345_678,
            "zstd".to_string(),
            "sha256:abc123".to_string(),
            1_700_000_000,
            "station-77".to_string(),
            "oracle-42".to_string(),
            Some(valid_chain()),
        )
    }

    #[test]
    fn dataset_manifest_round_trip() {
        let manifest = valid_manifest();
        let json = serde_json::to_string(&manifest).expect("serialize manifest");
        let decoded: DatasetManifest = serde_json::from_str(&json).expect("deserialize manifest");
        assert_eq!(manifest, decoded);
    }

    #[test]
    fn manifest_cid_is_deterministic() {
        let manifest = valid_manifest();
        assert_eq!(manifest.manifest_cid(), manifest.clone().manifest_cid());
    }

    #[test]
    fn validator_accepts_valid_chain() {
        let validator = ProvenanceValidator::new(ProvenancePolicy::default());
        let (decision, reason) = validator.decide_admission(Some(&valid_chain()));
        assert_eq!(decision, AdmissionDecision::Accepted);
        assert!(reason.is_none());
    }

    #[test]
    fn validator_rejects_missing_chain_when_required() {
        let validator = ProvenanceValidator::new(ProvenancePolicy::default());
        let (decision, reason) = validator.decide_admission(None);
        assert_eq!(decision, AdmissionDecision::Rejected);
        assert!(reason.is_none());
    }

    #[test]
    fn validator_quarantines_invalid_chain() {
        let validator = ProvenanceValidator::new(ProvenancePolicy::default());
        let mut chain = valid_chain();
        chain.links[1].parent_hash = Some("wrong-parent".to_string());
        let (decision, reason) = validator.decide_admission(Some(&chain));
        assert_eq!(decision, AdmissionDecision::Quarantined);
        assert!(reason.is_some());
    }
}
