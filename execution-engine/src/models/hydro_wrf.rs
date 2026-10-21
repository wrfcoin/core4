use async_trait::async_trait;

use crate::error::ExecutionError;
use crate::models::WeatherModel;
use crate::types::{ModelOutput, ModelRunSpec, PreparedInputs, ResourceLimits};

use super::wrf_common::{
    PhysicsSuite, domain_config_from_spec, extract_wrf_outputs, generate_namelist,
    generate_namelist_wps, prepare_wrf_inputs,
};

pub struct HydroWrf {
    gfs_endpoint: String,
}

impl Default for HydroWrf {
    fn default() -> Self {
        Self {
            gfs_endpoint: "https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25.pl".to_string(),
        }
    }
}

impl HydroWrf {
    pub fn new(gfs_endpoint: String) -> Self {
        Self { gfs_endpoint }
    }
}

#[async_trait]
impl WeatherModel for HydroWrf {
    fn model_id(&self) -> &str {
        "hydro-wrf"
    }

    fn docker_image(&self) -> &str {
        "wrfcoin/hydro-wrf:latest"
    }

    fn default_resource_limits(&self) -> ResourceLimits {
        ResourceLimits {
            memory_bytes: 6 * 1024 * 1024 * 1024,
            cpu_shares: 2048,
            timeout_secs: 10800,
        }
    }

    async fn prepare_inputs(
        &self,
        spec: &ModelRunSpec,
        work_dir: &str,
    ) -> Result<PreparedInputs, ExecutionError> {
        let config = domain_config_from_spec(spec, PhysicsSuite::Midlatitude)?;
        let mut namelist = generate_namelist(&config);
        namelist.push_str(
            r#"&hydro
 hydro_opt = 1,
 channel_routing = 1,
 subrtswcrt = 1,
 ovrtswcrt = 1,
 gwbaseswcrt = 1,
 land_surface_model = 2,
/
"#,
        );
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
            "Hydro-WRF",
            vec![
                "RAINC".to_string(),
                "SMOIS".to_string(),
                "SFROFF".to_string(),
                "T2".to_string(),
                "UDROFF".to_string(),
            ],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hydro_wrf_model_id() {
        let model = HydroWrf::default();
        assert_eq!(model.model_id(), "hydro-wrf");
    }

    #[test]
    fn hydro_wrf_docker_image() {
        let model = HydroWrf::default();
        assert_eq!(model.docker_image(), "wrfcoin/hydro-wrf:latest");
    }

    #[tokio::test]
    async fn prepare_inputs_includes_hydro_section() {
        let model = HydroWrf::default();
        let spec = ModelRunSpec {
            run_id: "test-hydro".to_string(),
            model_id: "hydro-wrf".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 1,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({"skip_gfs_fetch": true}),
        };

        let tmp = tempfile::tempdir().unwrap();
        let work_dir = tmp.path().to_string_lossy().to_string();
        let prepared = model.prepare_inputs(&spec, &work_dir).await.unwrap();

        assert_eq!(prepared.steps.len(), 5);

        let namelist_path = format!("{}/run/namelist.input", work_dir);
        let content = tokio::fs::read_to_string(namelist_path).await.unwrap();
        assert!(content.contains("&hydro"));
        assert!(content.contains("channel_routing = 1"));
    }
}
