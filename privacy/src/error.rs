//! Error types for the privacy crate.

/// Result alias for privacy operations.
pub type PrivacyResult<T> = Result<T, PrivacyError>;

/// Errors produced by privacy operations.
#[derive(Debug, thiserror::Error)]
pub enum PrivacyError {
    #[error("invalid key: {0}")]
    InvalidKey(String),

    #[error("invalid commitment: {0}")]
    InvalidCommitment(String),

    #[error("range proof failed: {0}")]
    RangeProof(String),

    #[error("ring signature error: {0}")]
    RingSignature(String),

    #[error("stealth address error: {0}")]
    StealthAddress(String),

    #[error("encryption error: {0}")]
    Encryption(String),

    #[error("double spend detected: key image already used")]
    DoubleSpend,

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("{0}")]
    Other(String),
}

impl From<serde_json::Error> for PrivacyError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e.to_string())
    }
}

impl From<hex::FromHexError> for PrivacyError {
    fn from(e: hex::FromHexError) -> Self {
        Self::Serialization(e.to_string())
    }
}
