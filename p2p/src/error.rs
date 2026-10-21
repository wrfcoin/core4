//! Error types for the WRFCoin P2P networking layer.

use thiserror::Error;

/// Errors that can occur in the P2P networking layer.
#[derive(Error, Debug)]
pub enum P2pError {
    /// Transport-level error (TCP, Noise handshake, Yamux framing, etc.).
    #[error("Transport error: {0}")]
    Transport(String),

    /// Gossipsub publish / subscribe / scoring failure.
    #[error("Gossipsub error: {0}")]
    Gossipsub(String),

    /// Bincode (de)serialization failure on the wire format.
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    /// JSON (de)serialization failure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Kademlia DHT operation failure.
    #[error("DHT error: {0}")]
    Dht(String),

    /// The internal command or event channel was closed unexpectedly.
    #[error("Channel closed")]
    ChannelClosed,

    /// Swarm-level error (dialling, listening, behaviour wiring).
    #[error("Swarm error: {0}")]
    Swarm(String),

    /// Invalid or missing configuration value.
    #[error("Configuration error: {0}")]
    Config(String),

    /// A message payload exceeds the inline data threshold and no content
    /// reference was provided for offloading.
    #[error("Message too large: {0}")]
    MessageTooLarge(String),

    /// I/O error from the underlying transport or file system.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience alias used throughout the crate.
pub type P2pResult<T> = Result<T, P2pError>;
