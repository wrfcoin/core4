use async_trait::async_trait;

use crate::error::ExecutionError;
use crate::models::WeatherModel;
use crate::types::{ModelOutput, ModelRunSpec, PreparedInputs, ResourceLimits};

use super::wrf_common::{
    PhysicsSuite, domain_config_from_spec, extract_wrf_outputs, generate_namelist,
    generate_namelist_wps, prepare_wrf_inputs,
};

pub struct SoilModel {
    gfs_endpoint: String,
}

impl Default for SoilModel {
    fn default() -> Self {
        Self {
            gfs_endpoint: "https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25.pl".to_string(),
        }
    }
}

impl SoilModel {
    pub fn new(gfs_endpoint: String) -> Self {
        Self { gfs_endpoint }
    }
}

#[async_trait]
impl WeatherModel for SoilModel {
    fn model_id(&self) -> &str {
        "soil-model"
    }

    fn docker_image(&self) -> &str {
        "wrfcoin/soil-model:latest"
    }

    fn default_resource_limits(&self) -> ResourceLimits {
        ResourceLimits {
            memory_bytes: 4 * 1024 * 1024 * 1024,
            cpu_shares: 2048,
            timeout_secs: 3600,
        }
    }

    async fn prepare_inputs(
        &self,
        spec: &ModelRunSpec,
        work_dir: &str,
    ) -> Result<PreparedInputs, ExecutionError> {
        let config = domain_config_from_spec(spec, PhysicsSuite::HighRes)?;
        let mut namelist = generate_namelist(&config);
        namelist.push_str(
            r#"&soil
 sf_surface_physics = 2,
 num_soil_layers = 4,
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
            vec![
                format!("GFS_ENDPOINT={}", self.gfs_endpoint),
                "WRF_SOIL=1".to_string(),
            ],
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
            "Soil-WRF",
            vec![
                "SH2O".to_string(),
                "SMOIS".to_string(),
                "SOILTEMP".to_string(),
                "TSLB".to_string(),
            ],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soil_model_id() {
        let model = SoilModel::default();
        assert_eq!(model.model_id(), "soil-model");
    }

    #[test]
    fn soil_model_docker_image() {
        let model = SoilModel::default();
        assert_eq!(model.docker_image(), "wrfcoin/soil-model:latest");
    }

    #[tokio::test]
    async fn prepare_inputs_creates_wrf_step_chain() {
        let model = SoilModel::default();
        let spec = ModelRunSpec {
            run_id: "test-soil".to_string(),
            model_id: "soil-model".to_string(),
            region: "us-midwest".to_string(),
            horizon_hours: 6,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({"skip_gfs_fetch": true}),
        };

        let tmp = tempfile::tempdir().unwrap();
        let work_dir = tmp.path().to_string_lossy().to_string();
        let prepared = model.prepare_inputs(&spec, &work_dir).await.unwrap();

        assert_eq!(prepared.steps.len(), 5);
        assert!(prepared.env.iter().any(|e| e == "WRF_SOIL=1"));

        let namelist = tokio::fs::read_to_string(format!("{}/run/namelist.input", work_dir))
            .await
            .unwrap();
        assert!(namelist.contains("&soil"));
    }
}
