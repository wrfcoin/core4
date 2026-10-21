use crate::error::ExecutionError;
use crate::types::ResourceLimits;
use async_trait::async_trait;

/// Trait abstracting Docker container operations, mockable for testing.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ContainerRuntime: Send + Sync {
    /// Ensure a Docker image is available locally (pull if needed).
    async fn ensure_image(&self, image: &str) -> Result<(), ExecutionError>;

    /// Run a container with the given image, command, environment, bind mounts, and limits.
    /// Returns (exit_code, stdout_logs).
    async fn run_container(
        &self,
        image: &str,
        cmd: Vec<String>,
        env: Vec<String>,
        work_dir: &str,
        binds: Vec<String>,
        limits: &ResourceLimits,
    ) -> Result<(i64, String), ExecutionError>;

    /// Clean up a stopped container by name/ID.
    async fn cleanup(&self, container_id: &str) -> Result<(), ExecutionError>;
}

/// Production Docker runtime using bollard.
pub struct DockerRunner {
    client: bollard::Docker,
}

impl DockerRunner {
    /// Create a new DockerRunner using the default Docker socket.
    pub fn new() -> Result<Self, ExecutionError> {
        let client = bollard::Docker::connect_with_local_defaults()
            .map_err(|e| ExecutionError::Docker(e.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl ContainerRuntime for DockerRunner {
    async fn ensure_image(&self, image: &str) -> Result<(), ExecutionError> {
        use bollard::image::CreateImageOptions;
        use futures_util::StreamExt;

        let options = CreateImageOptions {
            from_image: image,
            ..Default::default()
        };

        let mut stream = self.client.create_image(Some(options), None, None);
        while let Some(result) = stream.next().await {
            result.map_err(|e| ExecutionError::Docker(format!("pull failed: {e}")))?;
        }
        Ok(())
    }

    async fn run_container(
        &self,
        image: &str,
        cmd: Vec<String>,
        env: Vec<String>,
        work_dir: &str,
        binds: Vec<String>,
        limits: &ResourceLimits,
    ) -> Result<(i64, String), ExecutionError> {
        use bollard::container::{
            Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions,
            StartContainerOptions, WaitContainerOptions,
        };
        use bollard::models::HostConfig;
        use futures_util::StreamExt;

        let container_name = format!("wrfcoin-exec-{}", unique_container_id());

        let host_config = HostConfig {
            memory: Some(limits.memory_bytes as i64),
            cpu_shares: Some(limits.cpu_shares as i64),
            binds: if binds.is_empty() { None } else { Some(binds) },
            ..Default::default()
        };

        let config = Config {
            image: Some(image.to_string()),
            cmd: Some(cmd),
            env: Some(env),
            working_dir: Some(work_dir.to_string()),
            host_config: Some(host_config),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: &container_name,
            platform: None,
        };

        let container = self
            .client
            .create_container(Some(options), config)
            .await
            .map_err(|e| ExecutionError::Docker(format!("create failed: {e}")))?;

        self.client
            .start_container(&container.id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| ExecutionError::Docker(format!("start failed: {e}")))?;

        // Wait for container to finish
        let mut wait_stream = self.client.wait_container(
            &container.id,
            Some(WaitContainerOptions {
                condition: "not-running",
            }),
        );

        // Wait for container with timeout
        let timeout_duration = std::time::Duration::from_secs(limits.timeout_secs);
        let wait_result = tokio::time::timeout(timeout_duration, async {
            let mut exit_code: i64 = -1;
            while let Some(result) = wait_stream.next().await {
                match result {
                    Ok(response) => {
                        exit_code = response.status_code;
                    }
                    Err(e) => {
                        return Err(ExecutionError::Docker(format!("wait failed: {e}")));
                    }
                }
            }
            Ok(exit_code)
        }).await;

        let exit_code = match wait_result {
            Ok(Ok(code)) => code,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                // Timeout — kill the container
                let _ = self.client.kill_container::<String>(&container.id, None).await;
                let _ = self.client.remove_container(
                    &container.id,
                    Some(RemoveContainerOptions { force: true, ..Default::default() }),
                ).await;
                return Err(ExecutionError::Timeout(limits.timeout_secs));
            }
        };

        // Collect logs
        let log_options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            ..Default::default()
        };

        let mut logs = String::new();
        let mut log_stream = self.client.logs(&container.id, Some(log_options));
        while let Some(result) = log_stream.next().await {
            match result {
                Ok(output) => logs.push_str(&output.to_string()),
                Err(_) => break,
            }
        }

        // Cleanup container
        let _ = self
            .client
            .remove_container(
                &container.id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        Ok((exit_code, logs))
    }

    async fn cleanup(&self, container_id: &str) -> Result<(), ExecutionError> {
        use bollard::container::RemoveContainerOptions;
        self.client
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(|e| ExecutionError::Docker(format!("cleanup failed: {e}")))?;
        Ok(())
    }
}

/// No-op container runtime for test environments where Docker is unavailable.
pub struct NoOpContainerRuntime;

#[async_trait]
impl ContainerRuntime for NoOpContainerRuntime {
    async fn ensure_image(&self, _image: &str) -> Result<(), ExecutionError> {
        Err(ExecutionError::Docker("NoOpContainerRuntime: Docker not available".to_string()))
    }

    async fn run_container(
        &self,
        _image: &str,
        _cmd: Vec<String>,
        _env: Vec<String>,
        _work_dir: &str,
        _binds: Vec<String>,
        _limits: &ResourceLimits,
    ) -> Result<(i64, String), ExecutionError> {
        Err(ExecutionError::Docker("NoOpContainerRuntime: Docker not available".to_string()))
    }

    async fn cleanup(&self, _container_id: &str) -> Result<(), ExecutionError> {
        Ok(())
    }
}

fn unique_container_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let random: u64 = rand::random();
    format!("{:x}-{:x}", ts & 0xFFFF_FFFF, random & 0xFFFF_FFFF)
}
