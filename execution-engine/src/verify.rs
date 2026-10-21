use wrfcoin_shared_protocol::blockchain::transaction::{ComputeReceipt, ForecastArtifactManifest};
use wrfcoin_shared_protocol::consensus::forecast_verification::{
    ForecastVerificationOutcome, ForecastVerifier,
};

/// Verifier that validates forecast manifests using execution-engine logic.
///
/// Performs content-level verification:
/// 1. Structural manifest validation (schema, hashes, variables).
/// 2. If a compute receipt is present, verifies hash-chain consistency
///    (`work_hash` must match `manifest.output_hash`).
/// 3. Computes a skill score based on manifest content (variable coverage
///    and forecast horizon).
#[derive(Debug)]
pub struct ExecutionEngineForecastVerifier;

impl ExecutionEngineForecastVerifier {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExecutionEngineForecastVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ForecastVerifier for ExecutionEngineForecastVerifier {
    fn verify_manifest(
        &self,
        manifest: &ForecastArtifactManifest,
        compute_receipt: Option<&ComputeReceipt>,
    ) -> ForecastVerificationOutcome {
        // 1. Structural validation (schema version, hash lengths, sorted variables).
        if manifest.validate().is_err() {
            return ForecastVerificationOutcome {
                verified: false,
                skill_score: 0.0,
            };
        }

        // 2. Hash-chain consistency: if a compute receipt attests the work,
        //    its work_hash must match the manifest's output_hash.
        if let Some(receipt) = compute_receipt {
            if receipt.work_hash != manifest.output_hash {
                return ForecastVerificationOutcome {
                    verified: false,
                    skill_score: 0.0,
                };
            }
        }

        // 3. Compute skill score from manifest properties.
        //    With a valid compute receipt proving real work, the base score is
        //    higher (the node actually ran the model). Without a receipt, the
        //    manifest alone provides a lower but non-baseline score.
        let has_receipt = compute_receipt.is_some();
        let base: f64 = if has_receipt { 0.5 } else { 0.3 };

        // Variable coverage bonus: each output variable adds 0.02, capped at 0.3.
        let var_bonus = (manifest.output_variables.len() as f64 * 0.02).min(0.3);

        // Horizon bonus: longer forecasts are harder. Each 6 hours above 6
        // adds 0.01, capped at 0.1.
        let horizon_bonus = if manifest.horizon_hours > 6 {
            (((manifest.horizon_hours - 6) as f64 / 6.0) * 0.01).min(0.1)
        } else {
            0.0
        };

        let skill = (base + var_bonus + horizon_bonus).min(1.0);

        ForecastVerificationOutcome {
            verified: true,
            skill_score: skill,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> ForecastArtifactManifest {
        ForecastArtifactManifest {
            schema_version: 1,
            model_id: "wrf-nano".to_string(),
            input_hash: "b".repeat(64),
            output_hash: "a".repeat(64),
            produced_at: 1000,
            region: "us-east".to_string(),
            horizon_hours: 24,
            input_variables: vec![
                "geopotential_height".to_string(),
                "pressure".to_string(),
                "temperature".to_string(),
            ],
            output_variables: vec![
                "humidity".to_string(),
                "precipitation".to_string(),
                "pressure".to_string(),
                "temperature".to_string(),
                "wind_u".to_string(),
                "wind_v".to_string(),
            ],
        }
    }

    fn sample_receipt() -> ComputeReceipt {
        ComputeReceipt {
            task_id: "run-1".to_string(),
            worker_id: "worker-1".to_string(),
            work_hash: "a".repeat(64), // matches manifest.output_hash
            work_factor: 1024,
            completed_at: 1000,
            nonce: "deadbeef".to_string(),
            signature: None,
        }
    }

    #[test]
    fn verified_with_matching_receipt() {
        let v = ExecutionEngineForecastVerifier::new();
        let manifest = sample_manifest();
        let receipt = sample_receipt();
        let outcome = v.verify_manifest(&manifest, Some(&receipt));
        assert!(outcome.verified);
        // base 0.5 + var_bonus 0.12 + horizon_bonus 0.03 = 0.65
        assert!(outcome.skill_score > 0.5, "score={}", outcome.skill_score);
    }

    #[test]
    fn verified_without_receipt() {
        let v = ExecutionEngineForecastVerifier::new();
        let manifest = sample_manifest();
        let outcome = v.verify_manifest(&manifest, None);
        assert!(outcome.verified);
        // base 0.3 + bonuses → lower than with receipt
        assert!(outcome.skill_score > 0.3, "score={}", outcome.skill_score);
        assert!(outcome.skill_score < 0.7, "score={}", outcome.skill_score);
    }

    #[test]
    fn rejected_when_receipt_hash_mismatch() {
        let v = ExecutionEngineForecastVerifier::new();
        let manifest = sample_manifest();
        let mut receipt = sample_receipt();
        receipt.work_hash = "f".repeat(64); // mismatch
        let outcome = v.verify_manifest(&manifest, Some(&receipt));
        assert!(!outcome.verified);
        assert_eq!(outcome.skill_score, 0.0);
    }

    #[test]
    fn rejected_when_manifest_invalid() {
        let v = ExecutionEngineForecastVerifier::new();
        let mut manifest = sample_manifest();
        manifest.schema_version = 0; // invalid
        let outcome = v.verify_manifest(&manifest, None);
        assert!(!outcome.verified);
    }

    #[test]
    fn skill_score_bounded_at_one() {
        let v = ExecutionEngineForecastVerifier::new();
        let mut manifest = sample_manifest();
        // Add many variables (sorted) to test cap
        let mut vars: Vec<String> = (0..100).map(|i| format!("var_{i:03}")).collect();
        vars.sort();
        manifest.output_variables = vars;
        manifest.horizon_hours = 168;
        let outcome = v.verify_manifest(&manifest, Some(&sample_receipt()));
        assert!(outcome.verified);
        assert!(outcome.skill_score <= 1.0);
    }
}
