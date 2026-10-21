// E2E Pipeline Test — No Mocks (issue #631)
//
// Proves the full chain: pipeline → Docker → manifest → verifier → consensus-ready score.
// Uses a real DockerRunner (bollard) against the Docker daemon.
//
// Requires Docker daemon + `docker-tests` feature:
//   cargo test -p wrfcoin-execution-engine \
//       --features docker-tests \
//       --test e2e_pipeline_no_mocks -- --nocapture
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
#[ignore = "this target's tests require --features docker-tests; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-execution-engine --features docker-tests --test e2e_pipeline_no_mocks"]
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
        verify::ExecutionEngineForecastVerifier,
    };
    use wrfcoin_shared_protocol::consensus::forecast_verification::ForecastVerifier;

    /// Lightweight WRF-stub model — uses alpine:latest (no real WRF image needed).
    /// Simulates a multi-variable forecast output.
    struct WrfStubModel;

    #[async_trait]
    impl WeatherModel for WrfStubModel {
        fn model_id(&self) -> &str {
            "wrf-stub-e2e"
        }
        fn docker_image(&self) -> &str {
            "alpine:latest"
        }
        fn default_resource_limits(&self) -> ResourceLimits {
            ResourceLimits {
                memory_bytes: 128 * 1024 * 1024,
                cpu_shares: 256,
                timeout_secs: 60,
            }
        }
        async fn prepare_inputs(
            &self,
            spec: &ModelRunSpec,
            _work_dir: &str,
        ) -> Result<PreparedInputs, ExecutionError> {
            // Two-step pipeline simulating WRF preprocessing + execution.
            Ok(PreparedInputs {
                steps: vec![
                    ContainerStep {
                        name: "preprocess".to_string(),
                        cmd: vec![
                            "sh".to_string(),
                            "-c".to_string(),
                            format!(
                                "echo 'preprocessing region={} horizon={}h OK'",
                                spec.region, spec.horizon_hours,
                            ),
                        ],
                    },
                    ContainerStep {
                        name: "wrf-run".to_string(),
                        cmd: vec![
                            "sh".to_string(),
                            "-c".to_string(),
                            format!(
                                "echo 'WRF run_id={} model={} completed OK'",
                                spec.run_id, spec.model_id,
                            ),
                        ],
                    },
                ],
                binds: vec![],
                env: vec![],
            })
        }
        async fn extract_outputs(
            &self,
            _work_dir: &str,
            logs: &str,
        ) -> Result<ModelOutput, ExecutionError> {
            if !logs.contains("completed OK") {
                return Err(ExecutionError::OutputParsing(
                    "expected 'completed OK' in container output".to_string(),
                ));
            }
            Ok(ModelOutput {
                output_hash: hash_input(logs.as_bytes()),
                variables: vec![
                    "humidity".to_string(),
                    "precipitation".to_string(),
                    "pressure".to_string(),
                    "temperature".to_string(),
                    "wind_u".to_string(),
                    "wind_v".to_string(),
                ],
                output_size_bytes: logs.len() as u64,
                output_paths: vec![],
            })
        }
    }

    /// Full pipeline round-trip: Docker → manifest → receipt → verifier → consensus-ready score.
    #[tokio::test]
    async fn e2e_pipeline_produces_verifiable_manifest_and_receipt() {
        // 1. Stand up a real DockerRunner.
        let runner = DockerRunner::new()
            .expect("Docker daemon must be reachable — is DOCKER_HOST set?");
        let pipeline = ExecutionPipeline::new(Arc::new(runner));

        // 2. Build a spec with a known configuration.
        let spec = ModelRunSpec {
            run_id: "e2e-631-full".to_string(),
            model_id: "wrf-stub-e2e".to_string(),
            region: "us-central".to_string(),
            horizon_hours: 24,
            resource_limits: WrfStubModel.default_resource_limits(),
            params: serde_json::json!({}),
        };

        // 3. Execute through the real pipeline — NO MockContainerRuntime.
        let result = pipeline
            .execute(&WrfStubModel, &spec)
            .await
            .expect("pipeline.execute() must not return Err");

        // 4. Assert execution succeeded.
        assert!(result.success, "model run must succeed; error: {:?}", result.error);
        assert_eq!(result.exit_code, 0, "container exit code must be 0");
        assert!(result.execution_time_secs > 0.0);

        let output = result.output.expect("ModelRunResult.output must be Some");
        assert!(!output.output_hash.is_empty(), "output_hash must be non-empty");
        assert!(output.output_size_bytes > 0);
        assert_eq!(output.variables.len(), 6, "expected 6 output variables");

        // 5. Build manifest from pipeline result.
        let input_hash = hash_input(b"e2e-631-namelist-stub");
        let manifest = build_forecast_manifest(&spec, &output, &input_hash);
        assert!(
            manifest.validate().is_ok(),
            "manifest must pass validation: {:?}",
            manifest.validate()
        );
        assert_eq!(manifest.model_id, "wrf-stub-e2e");
        assert_eq!(manifest.region, "us-central");
        assert_eq!(manifest.horizon_hours, 24);

        // 6. Build compute receipt attesting the work.
        let receipt = build_compute_receipt(&spec, &output, "e2e-worker-631");
        assert_eq!(receipt.task_id, "e2e-631-full");
        assert_eq!(receipt.worker_id, "e2e-worker-631");
        assert_eq!(
            receipt.work_hash, manifest.output_hash,
            "receipt.work_hash must match manifest.output_hash for hash-chain consistency"
        );

        // 7. Feed into ExecutionEngineForecastVerifier — the REAL verifier.
        let verifier = ExecutionEngineForecastVerifier::new();
        let outcome = verifier.verify_manifest(&manifest, Some(&receipt));
        assert!(
            outcome.verified,
            "real verifier must accept pipeline-produced manifest+receipt"
        );
        // With receipt: base 0.5 + var_bonus (6 vars × 0.02 = 0.12) + horizon_bonus > 0.5
        assert!(
            outcome.skill_score > 0.5,
            "skill_score ({}) should exceed 0.5 (receipt present + multi-variable forecast)",
            outcome.skill_score
        );
        // And definitely above the baseline of 0.25
        assert!(
            outcome.skill_score > 0.25,
            "skill_score ({}) must exceed baseline (0.25)",
            outcome.skill_score
        );

        println!(
            "[E2E-631 PASS] run_id={} execution_time={:.3}s skill_score={:.3} verified=true",
            result.run_id, result.execution_time_secs, outcome.skill_score
        );
    }

    /// Verify that a manifest WITHOUT a compute receipt still passes verification
    /// but with a lower skill score.
    #[tokio::test]
    async fn e2e_pipeline_manifest_without_receipt_gets_lower_score() {
        let runner = DockerRunner::new()
            .expect("Docker daemon must be reachable");
        let pipeline = ExecutionPipeline::new(Arc::new(runner));

        let spec = ModelRunSpec {
            run_id: "e2e-631-no-receipt".to_string(),
            model_id: "wrf-stub-e2e".to_string(),
            region: "eu-west".to_string(),
            horizon_hours: 12,
            resource_limits: WrfStubModel.default_resource_limits(),
            params: serde_json::json!({}),
        };

        let result = pipeline
            .execute(&WrfStubModel, &spec)
            .await
            .expect("pipeline.execute() must not return Err");
        assert!(result.success);

        let output = result.output.expect("output must be present");
        let input_hash = hash_input(b"e2e-631-no-receipt-namelist");
        let manifest = build_forecast_manifest(&spec, &output, &input_hash);
        let receipt = build_compute_receipt(&spec, &output, "e2e-worker");

        let verifier = ExecutionEngineForecastVerifier::new();

        let with_receipt = verifier.verify_manifest(&manifest, Some(&receipt));
        let without_receipt = verifier.verify_manifest(&manifest, None);

        assert!(with_receipt.verified);
        assert!(without_receipt.verified);
        assert!(
            with_receipt.skill_score > without_receipt.skill_score,
            "score with receipt ({}) should exceed score without ({})",
            with_receipt.skill_score,
            without_receipt.skill_score
        );

        println!(
            "[E2E-631 PASS] with_receipt={:.3} without_receipt={:.3}",
            with_receipt.skill_score, without_receipt.skill_score
        );
    }
}
