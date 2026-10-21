pub mod gfs_fetch;
pub mod hydro_wrf;
pub mod netcdf_parse;
pub mod soil_model;
pub mod water_quality;
pub mod wrf_chem;
pub mod wrf_common;
pub mod wrf_fire;
pub mod wrf_nano;

use crate::error::ExecutionError;
use crate::types::{ModelOutput, ModelRunSpec, PreparedInputs, ResourceLimits};
use async_trait::async_trait;

/// Trait for weather model implementations.
///
/// Each model knows how to prepare inputs, build Docker commands,
/// and parse outputs for its specific weather model type.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait WeatherModel: Send + Sync {
    /// Unique identifier for this model.
    fn model_id(&self) -> &str;

    /// Docker image to use for this model.
    fn docker_image(&self) -> &str;

    /// Default resource limits for this model.
    fn default_resource_limits(&self) -> ResourceLimits;

    /// Prepare input files and container steps for the model run.
    async fn prepare_inputs(
        &self,
        spec: &ModelRunSpec,
        work_dir: &str,
    ) -> Result<PreparedInputs, ExecutionError>;

    /// Parse model outputs from the working directory.
    async fn extract_outputs(
        &self,
        work_dir: &str,
        logs: &str,
    ) -> Result<ModelOutput, ExecutionError>;
}
