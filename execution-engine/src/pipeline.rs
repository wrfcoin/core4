use crate::docker::ContainerRuntime;
use crate::error::ExecutionError;
use crate::models::WeatherModel;
use crate::types::{ModelRunResult, ModelRunSpec};
use std::sync::Arc;
use std::time::Instant;

/// Orchestrates the full execution pipeline: prepare inputs -> run Docker steps -> extract outputs.
pub struct ExecutionPipeline<R: ContainerRuntime + ?Sized> {
    runtime: Arc<R>,
}

impl<R: ContainerRuntime + ?Sized> ExecutionPipeline<R> {
    pub fn new(runtime: Arc<R>) -> Self {
        Self { runtime }
    }

    /// Execute a model run end-to-end.
    ///
    /// Iterates over each `ContainerStep` returned by `prepare_inputs`,
    /// running them sequentially with shared bind mounts. Fails fast on
    /// non-zero exit from any step.
    pub async fn execute(
        &self,
        model: &dyn WeatherModel,
        spec: &ModelRunSpec,
    ) -> Result<ModelRunResult, ExecutionError> {
        spec.validate()?;
        let start = Instant::now();

        // Create a temporary working directory
        let work_dir = tempfile::tempdir()?;
        let work_path = work_dir.path().to_string_lossy().to_string();

        // Ensure Docker image is available
        self.runtime.ensure_image(model.docker_image()).await?;

        // Prepare model inputs (multi-step)
        let prepared = model.prepare_inputs(spec, &work_path).await?;

        let limits = if spec.resource_limits.timeout_secs > 0 {
            spec.resource_limits.clone()
        } else {
            model.default_resource_limits()
        };

        // Execute each step sequentially, fail fast on non-zero exit
        let mut all_logs = String::new();
        let mut last_exit_code: i64 = 0;
        for step in &prepared.steps {
            let (exit_code, logs) = self
                .runtime
                .run_container(
                    model.docker_image(),
                    step.cmd.clone(),
                    prepared.env.clone(),
                    &work_path,
                    prepared.binds.clone(),
                    &limits,
                )
                .await?;

            all_logs.push_str(&format!("--- {} ---\n{}\n", step.name, logs));
            last_exit_code = exit_code;

            if exit_code != 0 {
                let execution_time = start.elapsed().as_secs_f64();
                return Ok(ModelRunResult {
                    run_id: spec.run_id.clone(),
                    success: false,
                    exit_code,
                    execution_time_secs: execution_time,
                    output: None,
                    error: Some(format!(
                        "step '{}' exited with code {exit_code}",
                        step.name
                    )),
                });
            }
        }

        let execution_time = start.elapsed().as_secs_f64();

        // Extract outputs
        match model.extract_outputs(&work_path, &all_logs).await {
            Ok(output) => Ok(ModelRunResult {
                run_id: spec.run_id.clone(),
                success: true,
                exit_code: last_exit_code,
                execution_time_secs: execution_time,
                output: Some(output),
                error: None,
            }),
            Err(e) => Ok(ModelRunResult {
                run_id: spec.run_id.clone(),
                success: false,
                exit_code: last_exit_code,
                execution_time_secs: execution_time,
                output: None,
                error: Some(e.to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::MockContainerRuntime;
    use crate::types::{ContainerStep, ModelOutput, PreparedInputs, ResourceLimits};
    use std::sync::Arc;

    // A simple test model for the pipeline
    struct TestModel;

    #[async_trait::async_trait]
    impl WeatherModel for TestModel {
        fn model_id(&self) -> &str { "test-model" }
        fn docker_image(&self) -> &str { "test:latest" }
        fn default_resource_limits(&self) -> ResourceLimits { ResourceLimits::default() }

        async fn prepare_inputs(
            &self,
            _spec: &ModelRunSpec,
            _work_dir: &str,
        ) -> Result<PreparedInputs, ExecutionError> {
            Ok(PreparedInputs {
                steps: vec![ContainerStep {
                    name: "echo".to_string(),
                    cmd: vec!["echo".to_string(), "hello".to_string()],
                }],
                binds: vec![],
                env: vec![],
            })
        }

        async fn extract_outputs(
            &self,
            _work_dir: &str,
            _logs: &str,
        ) -> Result<ModelOutput, ExecutionError> {
            Ok(ModelOutput {
                output_hash: "abc123".to_string(),
                variables: vec!["temp".to_string()],
                output_size_bytes: 100,
                output_paths: vec![],
            })
        }
    }

    /// Two-step test model to verify multi-step execution
    struct TwoStepModel;

    #[async_trait::async_trait]
    impl WeatherModel for TwoStepModel {
        fn model_id(&self) -> &str { "two-step" }
        fn docker_image(&self) -> &str { "test:latest" }
        fn default_resource_limits(&self) -> ResourceLimits { ResourceLimits::default() }

        async fn prepare_inputs(
            &self,
            _spec: &ModelRunSpec,
            _work_dir: &str,
        ) -> Result<PreparedInputs, ExecutionError> {
            Ok(PreparedInputs {
                steps: vec![
                    ContainerStep {
                        name: "step-1".to_string(),
                        cmd: vec!["echo".to_string(), "step1".to_string()],
                    },
                    ContainerStep {
                        name: "step-2".to_string(),
                        cmd: vec!["echo".to_string(), "step2".to_string()],
                    },
                ],
                binds: vec!["/tmp/test:/data:rw".to_string()],
                env: vec!["FOO=bar".to_string()],
            })
        }

        async fn extract_outputs(
            &self,
            _work_dir: &str,
            _logs: &str,
        ) -> Result<ModelOutput, ExecutionError> {
            Ok(ModelOutput {
                output_hash: "def456".to_string(),
                variables: vec!["temp".to_string()],
                output_size_bytes: 200,
                output_paths: vec![],
            })
        }
    }

    #[tokio::test]
    async fn pipeline_success() {
        let mut mock_runtime = MockContainerRuntime::new();

        mock_runtime
            .expect_ensure_image()
            .returning(|_| Ok(()));

        mock_runtime
            .expect_run_container()
            .returning(|_, _, _, _, _, _| Ok((0, "success".to_string())));

        let pipeline = ExecutionPipeline::new(Arc::new(mock_runtime));
        let spec = ModelRunSpec {
            run_id: "run-1".to_string(),
            model_id: "test-model".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 24,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({}),
        };

        let result = pipeline.execute(&TestModel, &spec).await.unwrap();
        assert!(result.success);
        assert_eq!(result.exit_code, 0);
        assert!(result.output.is_some());
    }

    #[tokio::test]
    async fn pipeline_multi_step_success() {
        let mut mock_runtime = MockContainerRuntime::new();

        mock_runtime
            .expect_ensure_image()
            .returning(|_| Ok(()));

        mock_runtime
            .expect_run_container()
            .times(2)
            .returning(|_, _, _, _, _, _| Ok((0, "ok".to_string())));

        let pipeline = ExecutionPipeline::new(Arc::new(mock_runtime));
        let spec = ModelRunSpec {
            run_id: "run-multi".to_string(),
            model_id: "two-step".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 24,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({}),
        };

        let result = pipeline.execute(&TwoStepModel, &spec).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn pipeline_multi_step_fails_fast() {
        let mut mock_runtime = MockContainerRuntime::new();

        mock_runtime
            .expect_ensure_image()
            .returning(|_| Ok(()));

        // First step fails — second step should NOT be called
        mock_runtime
            .expect_run_container()
            .times(1)
            .returning(|_, _, _, _, _, _| Ok((1, "step1 failed".to_string())));

        let pipeline = ExecutionPipeline::new(Arc::new(mock_runtime));
        let spec = ModelRunSpec {
            run_id: "run-fail".to_string(),
            model_id: "two-step".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 24,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({}),
        };

        let result = pipeline.execute(&TwoStepModel, &spec).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("step-1"));
    }

    #[tokio::test]
    async fn pipeline_nonzero_exit() {
        let mut mock_runtime = MockContainerRuntime::new();

        mock_runtime
            .expect_ensure_image()
            .returning(|_| Ok(()));

        mock_runtime
            .expect_run_container()
            .returning(|_, _, _, _, _, _| Ok((1, "error output".to_string())));

        let pipeline = ExecutionPipeline::new(Arc::new(mock_runtime));
        let spec = ModelRunSpec {
            run_id: "run-2".to_string(),
            model_id: "test-model".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 24,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({}),
        };

        let result = pipeline.execute(&TestModel, &spec).await.unwrap();
        assert!(!result.success);
        assert_eq!(result.exit_code, 1);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn pipeline_image_pull_failure() {
        let mut mock_runtime = MockContainerRuntime::new();

        mock_runtime
            .expect_ensure_image()
            .returning(|_| Err(ExecutionError::Docker("pull failed".to_string())));

        let pipeline = ExecutionPipeline::new(Arc::new(mock_runtime));
        let spec = ModelRunSpec {
            run_id: "run-3".to_string(),
            model_id: "test-model".to_string(),
            region: "us-east".to_string(),
            horizon_hours: 24,
            resource_limits: ResourceLimits::default(),
            params: serde_json::json!({}),
        };

        let result = pipeline.execute(&TestModel, &spec).await;
        assert!(result.is_err());
    }
}
