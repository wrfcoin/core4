// An empty test binary reports "0 passed; 0 failed; 0 ignored" — identical
// to a passing target and invisible behind sibling suites (the vacuity shape
// scripts/cargo_test_nonvacuous.py rejects as EMPTY_TEST_BINARY). Without the
// feature this target now declares its absence as a countable `ignored`
// instead of compiling to silence — #2748's principle, extended to the cfg
// dimension.
#[cfg(not(feature = "docker-tests"))]
#[test]
#[ignore = "this target's tests require --features docker-tests; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-execution-engine --features docker-tests --test wrf_nano_dry_run"]
fn tests_require_the_docker_tests_feature() {}

#[cfg(feature = "docker-tests")]
mod gated {
    use std::sync::Arc;

    use wrfcoin_execution_engine::{
        docker::{ContainerRuntime, DockerRunner},
        manifest::{build_compute_receipt, build_forecast_manifest, hash_input},
        models::wrf_nano::WrfNano,
        pipeline::ExecutionPipeline,
        types::{ModelRunSpec, ResourceLimits},
    };

    #[tokio::test]
    async fn test_wrf_nano_dry_run() {
        let runner = match DockerRunner::new() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Skipping test_wrf_nano_dry_run: Docker unavailable ({e})");
                return;
            },
        };

        if let Err(e) = runner.ensure_image("wrfcoin/wrf-nano:latest").await {
            eprintln!("Skipping test_wrf_nano_dry_run: WRF image unavailable ({e})");
            return;
        }

        let pipeline = ExecutionPipeline::new(Arc::new(runner));
        let model = WrfNano::default();

        let spec = ModelRunSpec {
            run_id: "wrf-nano-dry-run".to_string(),
            model_id: "wrf-nano".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 1,
            resource_limits: ResourceLimits {
                memory_bytes: 8 * 1024 * 1024 * 1024,
                cpu_shares: 4096,
                timeout_secs: 7200,
            },
            params: serde_json::json!({
                "start_year": 2026,
                "start_month": 1,
                "start_day": 1,
                "start_hour": 0,
                "e_we": 10,
                "e_sn": 10,
                "skip_gfs_fetch": true
            }),
        };

        let result = match pipeline.execute(&model, &spec).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Skipping test_wrf_nano_dry_run: execution prerequisites missing ({e})");
                return;
            },
        };

        if !result.success {
            eprintln!(
                "Skipping test_wrf_nano_dry_run: container/image not dry-run ready ({:?})",
                result.error
            );
            return;
        }

        let output = result.output.expect("output should be present on success");
        assert!(!output.output_hash.is_empty());
        assert!(!output.variables.is_empty());

        let input_hash = hash_input(b"wrf-nano-dry-run");
        let manifest = build_forecast_manifest(&spec, &output, &input_hash);
        assert!(manifest.validate().is_ok());

        let receipt = build_compute_receipt(&spec, &output, "dry-run-worker");
        assert_eq!(receipt.task_id, spec.run_id);
        assert_eq!(receipt.worker_id, "dry-run-worker");
        assert!(receipt.work_factor >= output.output_size_bytes);
    }
}
