// TDD-2.1 (issue #557): Integration test for the Docker-based WRF execution harness.
//
// Requires Docker daemon + `docker-tests` feature:
//   cargo test -p wrfcoin-execution-engine \
//       --features docker-tests \
//       --test tdd_2_1_docker_harness -- --nocapture
//
// Gated: invisible to standard CI without --features docker-tests.

// An empty test binary reports "0 passed; 0 failed; 0 ignored" — identical
// to a passing target and invisible behind sibling suites (the vacuity shape
// scripts/cargo_test_nonvacuous.py rejects as EMPTY_TEST_BINARY). Without the
// feature this target now declares its absence as a countable `ignored`
// instead of compiling to silence — #2748's principle, extended to the cfg
// dimension.
#[cfg(not(feature = "docker-tests"))]
#[test]
#[ignore = "this target's tests require --features docker-tests; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-execution-engine --features docker-tests --test tdd_2_1_docker_harness"]
fn tests_require_the_docker_tests_feature() {}

#[cfg(feature = "docker-tests")]
mod gated {
    use async_trait::async_trait;
    use std::sync::Arc;
    use wrfcoin_execution_engine::{
        docker::DockerRunner,
        error::ExecutionError,
        manifest::{build_compute_receipt, build_forecast_manifest, hash_input},
        models::WeatherModel,
        pipeline::ExecutionPipeline,
        types::{ContainerStep, ModelOutput, ModelRunSpec, PreparedInputs, ResourceLimits},
    };

    /// Lightweight stub model — uses alpine:latest (always pullable, no WRF image needed).
    struct EchoModel;

    #[async_trait]
    impl WeatherModel for EchoModel {
        fn model_id(&self) -> &str { "echo-stub" }
        fn docker_image(&self) -> &str { "alpine:latest" }
        fn default_resource_limits(&self) -> ResourceLimits {
            ResourceLimits { memory_bytes: 128 * 1024 * 1024, cpu_shares: 256, timeout_secs: 60 }
        }
        async fn prepare_inputs(
            &self, spec: &ModelRunSpec, _work_dir: &str,
        ) -> Result<PreparedInputs, ExecutionError> {
            Ok(PreparedInputs {
                steps: vec![ContainerStep {
                    name: "echo-step".to_string(),
                    cmd: vec![
                        "sh".to_string(), "-c".to_string(),
                        format!("echo 'WRF run {} region {} OK'", spec.run_id, spec.region),
                    ],
                }],
                binds: vec![],
                env: vec![],
            })
        }
        async fn extract_outputs(
            &self, _work_dir: &str, logs: &str,
        ) -> Result<ModelOutput, ExecutionError> {
            if !logs.contains("OK") {
                return Err(ExecutionError::OutputParsing(
                    "expected 'OK' in container output".to_string(),
                ));
            }
            Ok(ModelOutput {
                output_hash: hash_input(logs.as_bytes()),
                variables: vec!["temperature".to_string(), "pressure".to_string()],
                output_size_bytes: logs.len() as u64,
                output_paths: vec![],
            })
        }
    }

    /// TDD-2.1: Submit a model run to the Docker harness; verify a valid execution
    /// receipt (manifest + compute receipt) comes back.
    #[tokio::test]
    async fn tdd_2_1_docker_harness_submit_and_receipt() {
        let runner = DockerRunner::new()
            .expect("Docker daemon must be reachable — is DOCKER_HOST set?");
        let pipeline = ExecutionPipeline::new(Arc::new(runner));

        let spec = ModelRunSpec {
            run_id: "tdd-2-1-gate".to_string(),
            model_id: "echo-stub".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 6,
            resource_limits: EchoModel.default_resource_limits(),
            params: serde_json::json!({}),
        };

        // Execute
        let result = pipeline.execute(&EchoModel, &spec).await
            .expect("pipeline.execute() must not return Err");

        // Execution succeeded
        assert!(result.success, "model run must succeed; error: {:?}", result.error);
        assert_eq!(result.exit_code, 0, "container exit code must be 0");
        assert!(result.execution_time_secs > 0.0);

        let output = result.output.expect("ModelRunResult.output must be Some");
        assert!(!output.output_hash.is_empty());
        assert!(output.output_size_bytes > 0);

        // Manifest round-trip
        let input_hash = hash_input(b"tdd-2-1-namelist-stub");
        let manifest = build_forecast_manifest(&spec, &output, &input_hash);
        assert!(
            manifest.validate().is_ok(),
            "manifest must pass shared-protocol validation: {:?}", manifest.validate()
        );
        assert_eq!(manifest.model_id, "echo-stub");
        assert_eq!(manifest.region, "us-east");
        assert_eq!(manifest.horizon_hours, 6);
        assert!(!manifest.output_hash.is_empty());

        // Compute receipt
        let receipt = build_compute_receipt(&spec, &output, "testnet-worker-1");
        assert_eq!(receipt.task_id, "tdd-2-1-gate");
        assert_eq!(receipt.worker_id, "testnet-worker-1");
        assert!(!receipt.work_hash.is_empty());
        assert!(receipt.work_factor > 0);
        // Signing is deferred to the PoP consensus layer (ForecastVerificationReceipt),
        // not the execution-engine; no private key is available here.
        assert!(receipt.signature.is_none());

        println!(
            "[TDD-2.1 PASS] run_id={} execution_time={:.3}s manifest_valid=true",
            result.run_id, result.execution_time_secs
        );
    }
}
