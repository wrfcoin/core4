use thiserror::Error;

#[derive(Debug, Error)]
pub enum ZkError {
    #[error("proof generation failed: {0}")]
    ProofGenerationFailed(String),
    #[error("proof verification failed: {0}")]
    VerificationFailed(String),
    #[error("key generation failed: {0}")]
    KeyGenerationFailed(String),
    #[error("serialization error: {0}")]
    SerializationError(String),
    #[error("invalid circuit input: {0}")]
    InvalidInput(String),
}
