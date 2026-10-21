/// Integration tests for the WRF-Nano execution harness.
///
/// These tests require Docker and the `wrfcoin/wrf-nano:latest` image.
/// Build the image first:
///
/// ```sh
/// docker build -t wrfcoin/wrf-nano:latest -f ../infra/infra/docker/Dockerfile.wrf-nano .
/// ```
///
/// Run with: `cargo test -p wrfcoin-execution-engine -- --ignored`
use wrfcoin_execution_engine::docker::DockerRunner;
use wrfcoin_execution_engine::manifest::{build_forecast_manifest, hash_input};
use wrfcoin_execution_engine::models::wrf_nano::WrfNano;
use wrfcoin_execution_engine::pipeline::ExecutionPipeline;
use wrfcoin_execution_engine::registry::default_registry;
use wrfcoin_execution_engine::types::{ModelRunSpec, ResourceLimits};
use std::sync::Arc;

/// End-to-end Docker integration test.
///
/// Runs real.exe + wrf.exe inside the WRF-Nano container, extracts output,
/// and verifies a valid `ForecastArtifactManifest` can be built.
#[tokio::test]
#[ignore]
async fn wrf_nano_docker_e2e() {
    let runner = DockerRunner::new().expect("Docker must be available");
    let pipeline = ExecutionPipeline::new(Arc::new(runner));
    let model = WrfNano::default();

    let spec = ModelRunSpec {
        run_id: "integration-test-1".to_string(),
        model_id: "wrf-nano".to_string(),
        region: "us-east".to_string(),
        horizon_hours: 1,
        resource_limits: ResourceLimits {
            memory_bytes: 4 * 1024 * 1024 * 1024,
            cpu_shares: 2048,
            timeout_secs: 3600,
        },
        params: serde_json::json!({
            "start_year": 2026,
            "start_month": 1,
            "start_day": 1,
            "start_hour": 0,
            "e_we": 10,
            "e_sn": 10,
        }),
    };

    let result = pipeline.execute(&model, &spec).await.expect("execute should not error");

    assert!(result.success, "WRF run should succeed: {:?}", result.error);
    let output = result.output.expect("output should be present");
    assert!(!output.output_hash.is_empty(), "output hash must be non-empty");
    assert!(output.output_size_bytes > 0, "output should have nonzero size");

    // Build manifest and verify it passes shared-protocol validation
    let input_hash = hash_input(b"integration-test-namelist");
    let manifest = build_forecast_manifest(&spec, &output, &input_hash);
    assert!(
        manifest.validate().is_ok(),
        "manifest validation failed: {:?}",
        manifest.validate()
    );
}

/// Verify the default model registry contains all models.
#[test]
fn registry_has_all_models() {
    let registry = default_registry();
    let models = registry.list_models();
    assert!(models.contains(&"wrf-nano".to_string()));
    assert!(models.contains(&"wrf-chem".to_string()));
    assert!(models.contains(&"wrf-fire".to_string()));
    assert!(models.contains(&"hydro-wrf".to_string()));
    assert!(models.contains(&"soil-model".to_string()));
    assert!(models.contains(&"water-quality".to_string()));
    assert_eq!(models.len(), 6);
}

/// End-to-end Docker integration test for WRF-Chem.
#[tokio::test]
#[ignore]
async fn wrf_chem_docker_e2e() {
    use wrfcoin_execution_engine::models::wrf_chem::WrfChem;

    let runner = DockerRunner::new().expect("Docker must be available");
    let pipeline = ExecutionPipeline::new(Arc::new(runner));
    let model = WrfChem::default();

    let spec = ModelRunSpec {
        run_id: "integration-test-chem".to_string(),
        model_id: "wrf-chem".to_string(),
        region: "us-east".to_string(),
        horizon_hours: 1,
        resource_limits: ResourceLimits {
            memory_bytes: 8 * 1024 * 1024 * 1024,
            cpu_shares: 4096,
            timeout_secs: 3600,
        },
        params: serde_json::json!({
            "start_year": 2026, "start_month": 1, "start_day": 1, "start_hour": 0,
            "e_we": 10, "e_sn": 10,
        }),
    };

    let result = pipeline.execute(&model, &spec).await.expect("execute should not error");
    assert!(result.success, "WRF-Chem run should succeed: {:?}", result.error);
    let output = result.output.expect("output should be present");
    assert!(!output.output_hash.is_empty());
    assert!(output.variables.contains(&"o3".to_string()));
}

/// End-to-end Docker integration test for WRF-Fire.
#[tokio::test]
#[ignore]
async fn wrf_fire_docker_e2e() {
    use wrfcoin_execution_engine::models::wrf_fire::WrfFire;

    let runner = DockerRunner::new().expect("Docker must be available");
    let pipeline = ExecutionPipeline::new(Arc::new(runner));
    let model = WrfFire::default();

    let spec = ModelRunSpec {
        run_id: "integration-test-fire".to_string(),
        model_id: "wrf-fire".to_string(),
        region: "us-west".to_string(),
        horizon_hours: 1,
        resource_limits: ResourceLimits {
            memory_bytes: 8 * 1024 * 1024 * 1024,
            cpu_shares: 4096,
            timeout_secs: 3600,
        },
        params: serde_json::json!({
            "start_year": 2026, "start_month": 7, "start_day": 15, "start_hour": 12,
            "e_we": 10, "e_sn": 10,
            "fire_ignition_start_lon1": -120.5, "fire_ignition_start_lat1": 37.8,
        }),
    };

    let result = pipeline.execute(&model, &spec).await.expect("execute should not error");
    assert!(result.success, "WRF-Fire run should succeed: {:?}", result.error);
    let output = result.output.expect("output should be present");
    assert!(!output.output_hash.is_empty());
    assert!(output.variables.contains(&"fire_area".to_string()));
}

/// End-to-end Docker integration test for Hydro-WRF.
#[tokio::test]
#[ignore]
async fn hydro_wrf_docker_e2e() {
    use wrfcoin_execution_engine::models::hydro_wrf::HydroWrf;

    let runner = DockerRunner::new().expect("Docker must be available");
    let pipeline = ExecutionPipeline::new(Arc::new(runner));
    let model = HydroWrf::default();

    let spec = ModelRunSpec {
        run_id: "integration-test-hydro".to_string(),
        model_id: "hydro-wrf".to_string(),
        region: "us-gulf".to_string(),
        horizon_hours: 1,
        resource_limits: ResourceLimits {
            memory_bytes: 6 * 1024 * 1024 * 1024,
            cpu_shares: 2048,
            timeout_secs: 3600,
        },
        params: serde_json::json!({
            "start_year": 2026, "start_month": 3, "start_day": 1, "start_hour": 0,
            "e_we": 10, "e_sn": 10,
        }),
    };

    let result = pipeline.execute(&model, &spec).await.expect("execute should not error");
    assert!(result.success, "Hydro-WRF run should succeed: {:?}", result.error);
    let output = result.output.expect("output should be present");
    assert!(!output.output_hash.is_empty());
    assert!(output.variables.contains(&"streamflow".to_string()));
}

/// End-to-end Docker integration test for Soil Model.
#[tokio::test]
#[ignore]
async fn soil_model_docker_e2e() {
    use wrfcoin_execution_engine::models::soil_model::SoilModel;

    let runner = DockerRunner::new().expect("Docker must be available");
    let pipeline = ExecutionPipeline::new(Arc::new(runner));
    let model = SoilModel::default();

    let spec = ModelRunSpec {
        run_id: "integration-test-soil".to_string(),
        model_id: "soil-model".to_string(),
        region: "us-midwest".to_string(),
        horizon_hours: 24,
        resource_limits: ResourceLimits {
            memory_bytes: 4 * 1024 * 1024 * 1024,
            cpu_shares: 2048,
            timeout_secs: 3600,
        },
        params: serde_json::json!({}),
    };

    let result = pipeline.execute(&model, &spec).await.expect("execute should not error");
    assert!(result.success, "Soil model run should succeed: {:?}", result.error);
    let output = result.output.expect("output should be present");
    assert!(!output.output_hash.is_empty());
    assert!(output.variables.contains(&"soil_moisture".to_string()));
}

/// End-to-end Docker integration test for Water Quality.
#[tokio::test]
#[ignore]
async fn water_quality_docker_e2e() {
    use wrfcoin_execution_engine::models::water_quality::WaterQuality;

    let runner = DockerRunner::new().expect("Docker must be available");
    let pipeline = ExecutionPipeline::new(Arc::new(runner));
    let model = WaterQuality::default();

    let spec = ModelRunSpec {
        run_id: "integration-test-water".to_string(),
        model_id: "water-quality".to_string(),
        region: "us-east".to_string(),
        horizon_hours: 24,
        resource_limits: ResourceLimits {
            memory_bytes: 2 * 1024 * 1024 * 1024,
            cpu_shares: 1024,
            timeout_secs: 1800,
        },
        params: serde_json::json!({}),
    };

    let result = pipeline.execute(&model, &spec).await.expect("execute should not error");
    assert!(result.success, "Water quality run should succeed: {:?}", result.error);
    let output = result.output.expect("output should be present");
    assert!(!output.output_hash.is_empty());
    assert!(output.variables.contains(&"chlorine".to_string()));
}
