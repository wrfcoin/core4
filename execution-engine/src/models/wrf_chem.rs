use async_trait::async_trait;

use crate::error::ExecutionError;
use crate::models::WeatherModel;
use crate::types::{ModelOutput, ModelRunSpec, PreparedInputs, ResourceLimits};

use super::wrf_common::{
    PhysicsSuite, domain_config_from_spec, extract_wrf_outputs, generate_namelist,
    generate_namelist_wps, prepare_wrf_inputs,
};

pub struct WrfChem {
    gfs_endpoint: String,
}

impl Default for WrfChem {
    fn default() -> Self {
        Self {
            gfs_endpoint: "https://nomads.ncep.noaa.gov/cgi-bin/filter_gfs_0p25.pl".to_string(),
        }
    }
}

impl WrfChem {
    pub fn new(gfs_endpoint: String) -> Self {
        Self { gfs_endpoint }
    }
}

#[async_trait]
impl WeatherModel for WrfChem {
    fn model_id(&self) -> &str {
        "wrf-chem"
    }

    fn docker_image(&self) -> &str {
        "wrfcoin/wrf-chem:latest"
    }

    fn default_resource_limits(&self) -> ResourceLimits {
        ResourceLimits {
            memory_bytes: 8 * 1024 * 1024 * 1024,
            cpu_shares: 4096,
            timeout_secs: 14400,
        }
    }

    async fn prepare_inputs(
        &self,
        spec: &ModelRunSpec,
        work_dir: &str,
    ) -> Result<PreparedInputs, ExecutionError> {
        let config = domain_config_from_spec(spec, PhysicsSuite::Tropical)?;
        let mut namelist = generate_namelist(&config);
        namelist.push_str(
            r#"&chem
 chem_opt = 1,
 chemdt = 10,
 emiss_opt = 3,
 bio_emiss_opt = 1,
 gas_drydep_opt = 1,
 aer_drydep_opt = 1,
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
                "WRF_CHEM=1".to_string(),
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
            "WRF-Chem",
            vec![
                "CO".to_string(),
                "NO2".to_string(),
                "O3".to_string(),
                "PM2_5_DRY".to_string(),
                "SO2".to_string(),
                "T2".to_string(),
            ],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrf_chem_model_id() {
        let model = WrfChem::default();
        assert_eq!(model.model_id(), "wrf-chem");
    }

    #[test]
    fn wrf_chem_docker_image() {
        let model = WrfChem::default();
        assert_eq!(model.docker_image(), "wrfcoin/wrf-chem:latest");
    }

    #[test]
    fn wrf_chem_default_limits() {
        let model = WrfChem::default();
        let limits = model.default_resource_limits();
        assert_eq!(limits.memory_bytes, 8 * 1024 * 1024 * 1024);
        assert_eq!(limits.cpu_shares, 4096);
        assert_eq!(limits.timeout_secs, 14400);
    }

    #[tokio::test]
    async fn prepare_inputs_includes_chem_and_wps() {
        let model = WrfChem::default();
        let spec = ModelRunSpec {
            run_id: "test-steps".to_string(),
            model_id: "wrf-chem".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 1,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({"skip_gfs_fetch": true}),
        };
        let tmp = tempfile::tempdir().unwrap();
        let work_dir = tmp.path().to_string_lossy().to_string();
        let prepared = model.prepare_inputs(&spec, &work_dir).await.unwrap();

        assert_eq!(prepared.steps.len(), 5);
        assert!(prepared.env.iter().any(|e| e == "WRF_CHEM=1"));

        let namelist_path = format!("{}/run/namelist.input", work_dir);
        let content = tokio::fs::read_to_string(&namelist_path).await.unwrap();
        assert!(content.contains("&chem"));

        let wps_path = format!("{}/run/namelist.wps", work_dir);
        assert!(tokio::fs::metadata(wps_path).await.is_ok());
    }
}
