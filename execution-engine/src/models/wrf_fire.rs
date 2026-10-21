use async_trait::async_trait;

use crate::error::ExecutionError;
use crate::models::WeatherModel;
use crate::types::{ModelOutput, ModelRunSpec, PreparedInputs, ResourceLimits};

use super::wrf_common::{
    PhysicsSuite, domain_config_from_spec, extract_wrf_outputs, generate_namelist,
    generate_namelist_wps, prepare_wrf_inputs,
};

pub struct WrfFire {
    gfs_endpoint: String,
}

impl Default for WrfFire {
    fn default() -> Self {
        Self {
            gfs_endpoint: "https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25.pl".to_string(),
        }
    }
}

impl WrfFire {
    pub fn new(gfs_endpoint: String) -> Self {
        Self { gfs_endpoint }
    }
}

#[async_trait]
impl WeatherModel for WrfFire {
    fn model_id(&self) -> &str {
        "wrf-fire"
    }

    fn docker_image(&self) -> &str {
        "wrfcoin/wrf-fire:latest"
    }

    fn default_resource_limits(&self) -> ResourceLimits {
        ResourceLimits {
            memory_bytes: 8 * 1024 * 1024 * 1024,
            cpu_shares: 4096,
            timeout_secs: 10800,
        }
    }

    async fn prepare_inputs(
        &self,
        spec: &ModelRunSpec,
        work_dir: &str,
    ) -> Result<PreparedInputs, ExecutionError> {
        let config = domain_config_from_spec(spec, PhysicsSuite::Fire)?;
        let ignition_lon = spec
            .params
            .get("fire_ignition_start_lon1")
            .and_then(|v| v.as_f64())
            .unwrap_or(config.center_lon);
        let ignition_lat = spec
            .params
            .get("fire_ignition_start_lat1")
            .and_then(|v| v.as_f64())
            .unwrap_or(config.center_lat);

        let mut namelist = generate_namelist(&config);
        namelist.push_str(&format!(
            r#"&fire
 ifire = 2,
 fire_fuel_read = 0,
 fire_num_ignitions = 1,
 fire_ignition_start_lon1 = {ignition_lon},
 fire_ignition_start_lat1 = {ignition_lat},
 fire_ignition_radius1 = 500,
 fmoist_run = .true.,
 fmoist_interp = .true.,
/
"#
        ));
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
            "WRF-Fire",
            vec![
                "FGRNHFX".to_string(),
                "FIRE_AREA".to_string(),
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
    fn wrf_fire_model_id() {
        let model = WrfFire::default();
        assert_eq!(model.model_id(), "wrf-fire");
    }

    #[test]
    fn wrf_fire_docker_image() {
        let model = WrfFire::default();
        assert_eq!(model.docker_image(), "wrfcoin/wrf-fire:latest");
    }

    #[tokio::test]
    async fn prepare_inputs_includes_fire_section() {
        let model = WrfFire::default();
        let spec = ModelRunSpec {
            run_id: "test-fire".to_string(),
            model_id: "wrf-fire".to_string(),
            region: "us-west".to_string(),
            horizon_hours: 1,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({
                "skip_gfs_fetch": true,
                "fire_ignition_start_lon1": -120.5,
                "fire_ignition_start_lat1": 37.8
            }),
        };

        let tmp = tempfile::tempdir().unwrap();
        let work_dir = tmp.path().to_string_lossy().to_string();
        let prepared = model.prepare_inputs(&spec, &work_dir).await.unwrap();

        assert_eq!(prepared.steps.len(), 5);
        assert_eq!(prepared.steps[0].name, "geogrid.exe");

        let namelist_path = format!("{}/run/namelist.input", work_dir);
        let content = tokio::fs::read_to_string(namelist_path).await.unwrap();
        assert!(content.contains("&fire"));
        assert!(content.contains("fire_ignition_start_lon1 = -120.5"));
        assert!(content.contains("fire_ignition_start_lat1 = 37.8"));
    }
}
