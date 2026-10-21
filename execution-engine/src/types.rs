use serde::{Deserialize, Serialize};

/// Specification for a model run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRunSpec {
    /// Unique run identifier.
    pub run_id: String,
    /// Model identifier (e.g., "wrf-nano", "wrf-chem").
    pub model_id: String,
    /// Geographic region for this run.
    pub region: String,
    /// Forecast horizon in hours.
    pub horizon_hours: u32,
    /// Resource limits for the container.
    pub resource_limits: ResourceLimits,
    /// Additional parameters passed to the model.
    pub params: serde_json::Value,
}

impl ModelRunSpec {
    /// Validate that spec fields don't contain potentially dangerous characters.
    /// Docker env vars are passed structurally (not via shell), but we validate
    /// at the boundary for defense in depth.
    pub fn validate(&self) -> Result<(), crate::error::ExecutionError> {
        // Alphanumeric, hyphens, underscores, dots, and slashes only
        let safe_pattern = |s: &str| s.chars().all(|c| c.is_alphanumeric() || "-_./:".contains(c));

        if !safe_pattern(&self.run_id) {
            return Err(crate::error::ExecutionError::Model(
                "run_id contains invalid characters".to_string(),
            ));
        }
        if !safe_pattern(&self.model_id) {
            return Err(crate::error::ExecutionError::Model(
                "model_id contains invalid characters".to_string(),
            ));
        }
        if !safe_pattern(&self.region) {
            return Err(crate::error::ExecutionError::Model(
                "region contains invalid characters".to_string(),
            ));
        }
        Ok(())
    }
}

/// Resource limits for a Docker container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory in bytes.
    pub memory_bytes: u64,
    /// CPU shares (relative weight).
    pub cpu_shares: u64,
    /// Maximum execution time in seconds.
    pub timeout_secs: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 2 * 1024 * 1024 * 1024, // 2 GB
            cpu_shares: 1024,
            timeout_secs: 3600, // 1 hour
        }
    }
}

/// A single step in a multi-step container execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStep {
    /// Human-readable name (e.g. "real.exe", "wrf.exe").
    pub name: String,
    /// Command and arguments to execute.
    pub cmd: Vec<String>,
}

/// Prepared inputs for a model run, supporting multi-step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedInputs {
    /// Ordered list of container steps to execute.
    pub steps: Vec<ContainerStep>,
    /// Bind mounts shared across all steps (e.g. `["/host/run:/wrf/run:rw"]`).
    pub binds: Vec<String>,
    /// Environment variables shared across all steps.
    pub env: Vec<String>,
}

/// Result of a model execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRunResult {
    /// The run specification that produced this result.
    pub run_id: String,
    /// Whether the run succeeded.
    pub success: bool,
    /// Container exit code.
    pub exit_code: i64,
    /// Execution wall-clock time in seconds.
    pub execution_time_secs: f64,
    /// Model output data.
    pub output: Option<ModelOutput>,
    /// Error message if the run failed.
    pub error: Option<String>,
}

/// Output from a model run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOutput {
    /// Hash of the output data (blake3).
    pub output_hash: String,
    /// Output variables produced by the model.
    pub variables: Vec<String>,
    /// Size of the output in bytes.
    pub output_size_bytes: u64,
    /// Path to output file(s) inside the container.
    pub output_paths: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_validate_rejects_shell_chars() {
        let spec = ModelRunSpec {
            run_id: "run; rm -rf /".to_string(),
            model_id: "test".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 24,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({}),
        };
        assert!(spec.validate().is_err());
    }

    #[test]
    fn spec_validate_accepts_valid() {
        let spec = ModelRunSpec {
            run_id: "run-123".to_string(),
            model_id: "wrf-nano".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 24,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({}),
        };
        assert!(spec.validate().is_ok());
    }
}
