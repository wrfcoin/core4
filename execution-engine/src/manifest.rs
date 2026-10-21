use crate::types::{ModelOutput, ModelRunSpec};
use wrfcoin_shared_protocol::blockchain::transaction::{ComputeReceipt, ForecastArtifactManifest};

/// Build a `ForecastArtifactManifest` from execution-engine types.
///
/// The `input_hash` should be a blake3 hex digest of the namelist / boundary
/// condition inputs that were fed into the model run.
pub fn build_forecast_manifest(
    spec: &ModelRunSpec,
    output: &ModelOutput,
    input_hash: &str,
) -> ForecastArtifactManifest {
    let mut input_variables = vec![
        "geopotential_height".to_string(),
        "pressure".to_string(),
        "relative_humidity".to_string(),
        "temperature".to_string(),
        "wind_u".to_string(),
        "wind_v".to_string(),
    ];
    input_variables.sort();
    input_variables.dedup();

    let mut output_variables = output.variables.clone();
    output_variables.sort();
    output_variables.dedup();

    ForecastArtifactManifest {
        schema_version: 1,
        model_id: spec.model_id.clone(),
        input_hash: input_hash.to_string(),
        output_hash: output.output_hash.clone(),
        produced_at: chrono::Utc::now().timestamp(),
        region: spec.region.clone(),
        horizon_hours: spec.horizon_hours,
        input_variables,
        output_variables,
    }
}

/// Build a `ComputeReceipt` attesting that a model run was executed.
pub fn build_compute_receipt(
    spec: &ModelRunSpec,
    output: &ModelOutput,
    worker_id: &str,
) -> ComputeReceipt {
    ComputeReceipt {
        task_id: spec.run_id.clone(),
        worker_id: worker_id.to_string(),
        work_hash: output.output_hash.clone(),
        work_factor: output.output_size_bytes,
        completed_at: chrono::Utc::now().timestamp(),
        nonce: format!("{:x}", rand::random::<u64>()),
        signature: None,
    }
}

/// Compute a blake3 hex hash of a namelist string (used as `input_hash`).
pub fn hash_input(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelOutput, ModelRunSpec, ResourceLimits};

    fn sample_spec() -> ModelRunSpec {
        ModelRunSpec {
            run_id: "run-manifest-test".to_string(),
            model_id: "wrf-nano".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 24,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({}),
        }
    }

    fn sample_output() -> ModelOutput {
        ModelOutput {
            output_hash: "a".repeat(64),
            variables: vec![
                "humidity".to_string(),
                "precipitation".to_string(),
                "pressure".to_string(),
                "temperature".to_string(),
                "wind_u".to_string(),
                "wind_v".to_string(),
            ],
            output_size_bytes: 1024,
            output_paths: vec!["/data/wrfout_d01".to_string()],
        }
    }

    #[test]
    fn manifest_passes_validation() {
        let spec = sample_spec();
        let output = sample_output();
        let input_hash = "b".repeat(64);

        let manifest = build_forecast_manifest(&spec, &output, &input_hash);
        assert!(manifest.validate().is_ok(), "manifest should be valid");
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.model_id, "wrf-nano");
        assert_eq!(manifest.horizon_hours, 24);
        assert_eq!(manifest.region, "us-east");
    }

    #[test]
    fn manifest_variables_are_sorted() {
        let spec = sample_spec();
        let output = sample_output();
        let input_hash = "c".repeat(64);

        let manifest = build_forecast_manifest(&spec, &output, &input_hash);
        let iv = &manifest.input_variables;
        let ov = &manifest.output_variables;
        assert!(iv.windows(2).all(|w| w[0] <= w[1]), "input vars must be sorted");
        assert!(ov.windows(2).all(|w| w[0] <= w[1]), "output vars must be sorted");
    }

    #[test]
    fn compute_receipt_fields() {
        let spec = sample_spec();
        let output = sample_output();
        let receipt = build_compute_receipt(&spec, &output, "worker-1");

        assert_eq!(receipt.task_id, "run-manifest-test");
        assert_eq!(receipt.worker_id, "worker-1");
        assert_eq!(receipt.work_hash, "a".repeat(64));
        assert_eq!(receipt.work_factor, 1024);
        assert!(receipt.signature.is_none());
    }

    #[test]
    fn hash_input_deterministic() {
        let data = b"namelist contents";
        let h1 = hash_input(data);
        let h2 = hash_input(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }
}
