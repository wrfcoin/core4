use wrfcoin_execution_engine::docker::{ContainerRuntime, DockerRunner};
use wrfcoin_execution_engine::types::ResourceLimits;

#[tokio::test]
async fn test_basic_docker_execution() {
    let runner = DockerRunner::new().expect("Docker must be available");
    let image = "alpine:latest";

    println!("Pulling image: {}", image);
    runner
        .ensure_image(image)
        .await
        .expect("Should be able to pull alpine");

    let limits = ResourceLimits {
        memory_bytes: 128 * 1024 * 1024,
        cpu_shares: 256,
        timeout_secs: 30,
    };

    println!("Running container...");
    let (exit_code, logs) = runner
        .run_container(
            image,
            vec!["echo".to_string(), "hello from docker".to_string()],
            vec![],
            "/",
            vec![],
            &limits,
        )
        .await
        .expect("Should be able to run container");

    println!("Exit code: {}", exit_code);
    println!("Logs: {}", logs);

    assert_eq!(exit_code, 0);
    assert!(logs.contains("hello from docker"));
}
