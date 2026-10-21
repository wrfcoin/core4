use std::fmt::Debug;

use crate::blockchain::transaction::{ComputeReceipt, ForecastArtifactManifest};

/// Outcome of verifying a forecast manifest against execution-engine results.
#[derive(Debug, Clone)]
pub struct ForecastVerificationOutcome {
    /// Whether the manifest passed content-level verification.
    pub verified: bool,
    /// Computed skill score (0.0–1.0) based on manifest content.
    /// Only meaningful when `verified` is true.
    pub skill_score: f64,
}

/// Trait for verifying forecast manifests against real execution-engine output.
///
/// This trait keeps execution-engine dependencies out of shared-protocol
/// (same pattern as `ZkVerifier`). The real implementation lives in
/// execution-engine; consensus-engines injects it into `ProofOfPublishing`
/// at startup.
///
/// When no verifier is injected, PoP falls back to `BASELINE_FORECAST_SKILL`.
pub trait ForecastVerifier: Send + Sync + Debug {
    /// Verify a forecast manifest, optionally cross-checking against a
    /// compute receipt for hash-chain consistency.
    fn verify_manifest(
        &self,
        manifest: &ForecastArtifactManifest,
        compute_receipt: Option<&ComputeReceipt>,
    ) -> ForecastVerificationOutcome;
}

/// Default verifier that rejects all manifests (used when no real verifier
/// is injected). This ensures PoP falls back to `BASELINE_FORECAST_SKILL`
/// by default, preserving existing behavior.
#[derive(Debug)]
pub struct BaselineForecastVerifier;

impl ForecastVerifier for BaselineForecastVerifier {
    fn verify_manifest(
        &self,
        _manifest: &ForecastArtifactManifest,
        _compute_receipt: Option<&ComputeReceipt>,
    ) -> ForecastVerificationOutcome {
        ForecastVerificationOutcome {
            verified: false,
            skill_score: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest() -> ForecastArtifactManifest {
        ForecastArtifactManifest {
            schema_version: 1,
            model_id: "wrf-gfs-v3".to_string(),
            input_hash: "a".repeat(64),
            output_hash: "b".repeat(64),
            produced_at: 1_700_000_000,
            region: "us-east".to_string(),
            horizon_hours: 48,
            input_variables: vec!["temperature".to_string(), "humidity".to_string()],
            output_variables: vec!["precipitation".to_string()],
        }
    }

    fn test_receipt() -> ComputeReceipt {
        ComputeReceipt {
            task_id: "task-42".to_string(),
            worker_id: "node-1".to_string(),
            work_hash: "c".repeat(64),
            work_factor: 1000,
            completed_at: 1_700_000_100,
            nonce: "nonce-xyz".to_string(),
            signature: None,
        }
    }

    #[test]
    fn baseline_verifier_always_returns_unverified_with_zero_skill_score() {
        let v = BaselineForecastVerifier;
        let out = v.verify_manifest(&test_manifest(), None);
        assert!(!out.verified);
        assert_eq!(out.skill_score, 0.0);
    }

    #[test]
    fn baseline_verifier_ignores_compute_receipt_and_still_returns_unverified() {
        let v = BaselineForecastVerifier;
        let receipt = test_receipt();
        let out = v.verify_manifest(&test_manifest(), Some(&receipt));
        assert!(!out.verified, "baseline must reject even when receipt is provided");
        assert_eq!(out.skill_score, 0.0);
    }

    #[test]
    fn baseline_verifier_works_through_trait_object() {
        let v: Box<dyn ForecastVerifier> = Box::new(BaselineForecastVerifier);
        let out = v.verify_manifest(&test_manifest(), None);
        assert!(!out.verified);
        assert_eq!(out.skill_score, 0.0);
    }
}
