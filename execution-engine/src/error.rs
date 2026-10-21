use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("Docker error: {0}")]
    Docker(String),
    #[error("model error: {0}")]
    Model(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("input download failed: {0}")]
    InputDownload(String),
    #[error("output parsing failed: {0}")]
    OutputParsing(String),
    #[error("container timed out after {0}s")]
    Timeout(u64),
    #[error("container exited with non-zero code: {0}")]
    NonZeroExit(i64),
}
