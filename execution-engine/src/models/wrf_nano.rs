use async_trait::async_trait;

use crate::error::ExecutionError;
use crate::models::WeatherModel;
use crate::types::{ModelOutput, ModelRunSpec, PreparedInputs, ResourceLimits};

use super::wrf_common::{
    PhysicsSuite, domain_config_from_spec, extract_wrf_outputs, generate_namelist,
    generate_namelist_wps, prepare_wrf_inputs,
};

pub struct WrfNano {
    gfs_endpoint: String,
}

impl Default for WrfNano {
    fn default() -> Self {
        Self {
            gfs_endpoint: "https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25.pl".to_string(),
        }
    }
}

impl WrfNano {
    pub fn new(gfs_endpoint: String) -> Self {
        Self { gfs_endpoint }
    }
}

#[async_trait]
impl WeatherModel for WrfNano {
    fn model_id(&self) -> &str {
        "wrf-nano"
    }

    fn docker_image(&self) -> &str {
        "wrfcoin/wrf-nano:latest"
    }

    fn default_resource_limits(&self) -> ResourceLimits {
        ResourceLimits {
            memory_bytes: 8 * 1024 * 1024 * 1024,
            cpu_shares: 4096,
            timeout_secs: 7200,
        }
    }

    async fn prepare_inputs(
        &self,
        spec: &ModelRunSpec,
        work_dir: &str,
    ) -> Result<PreparedInputs, ExecutionError> {
        let config = domain_config_from_spec(spec, PhysicsSuite::Midlatitude)?;
        let namelist = generate_namelist(&config);
        let namelist_wps = generate_namelist_wps(&config);

        prepare_wrf_inputs(
            spec,
            work_dir,
            &config,
            &namelist,
            &namelist_wps,
            &self.gfs_endpoint,
            vec![format!("GFS_ENDPOINT={}", self.gfs_endpoint)],
        )
        .await
    }

    async fn extract_outputs(
        &self,
        work_dir: &str,
        logs: &str,
    ) -> Result<ModelOutput, ExecutionError> {
        extract_wrf_outputs(
            work_dir,
            logs,
            "WRF-Nano",
            vec![
                "PSFC".to_string(),
                "Q2".to_string(),
                "RAINC".to_string(),
                "T2".to_string(),
                "U10".to_string(),
                "V10".to_string(),
            ],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrf_nano_model_id() {
        let model = WrfNano::default();
        assert_eq!(model.model_id(), "wrf-nano");
    }

    #[test]
    fn wrf_nano_docker_image() {
        let model = WrfNano::default();
        assert_eq!(model.docker_image(), "wrfcoin/wrf-nano:latest");
    }

    #[test]
    fn wrf_nano_default_limits() {
        let model = WrfNano::default();
        let limits = model.default_resource_limits();
        assert_eq!(limits.memory_bytes, 8 * 1024 * 1024 * 1024);
        assert_eq!(limits.cpu_shares, 4096);
        assert_eq!(limits.timeout_secs, 7200);
    }

    #[tokio::test]
    async fn prepare_inputs_creates_wps_and_wrf_steps() {
        let model = WrfNano::default();
        let spec = ModelRunSpec {
            run_id: "test-steps".to_string(),
            model_id: "wrf-nano".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 1,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({"skip_gfs_fetch": true}),
        };
        let tmp = tempfile::tempdir().unwrap();
        let work_dir = tmp.path().to_string_lossy().to_string();
        let prepared = model.prepare_inputs(&spec, &work_dir).await.unwrap();

        assert_eq!(prepared.steps.len(), 5);
        assert_eq!(prepared.steps[0].name, "geogrid.exe");
        assert_eq!(prepared.steps[4].name, "wrf.exe");

        let namelist_path = format!("{}/run/namelist.input", work_dir);
        let wps_path = format!("{}/run/namelist.wps", work_dir);
        assert!(tokio::fs::metadata(namelist_path).await.is_ok());
        assert!(tokio::fs::metadata(wps_path).await.is_ok());
    }
}
