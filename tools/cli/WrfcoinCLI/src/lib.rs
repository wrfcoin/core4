// Wrfcoin CLI Library
//
// This crate provides the core functionality for the Wrfcoin CLI.

#![allow(missing_docs)]

//! # Wrfcoin CLI Library
//!
//! `wrfcoin_cli` is a comprehensive library for interacting with the Wrfcoin blockchain.
//! It provides functionality for managing wallets, sending transactions, monitoring the
//! blockchain, and managing NFTs, all through a command-line interface.
//!
//! The library includes advanced security features to protect against various attack vectors
//! such as 51% attacks, double spending, and Sybil attacks. The consensus module implements
//! mechanisms for maintaining blockchain integrity with fork resolution and difficulty adjustment.

pub mod api;
pub mod bond_grant;
pub mod forecast_receipt;
pub mod config;
pub mod consensus;
pub mod crypto;
pub mod db;
pub mod errors;
#[path = "commands/forecast_submission.rs"]
pub mod forecast_submission;
pub mod output;
pub mod node_operator;
pub mod security;
pub mod security_key;
pub mod websocket;
pub mod why_report;

// Re-export commonly used types
pub use api::{
    AddressInfo, ApiClient, ApiConfig, Block, FeeEstimate, MiningStats, NFT, NetworkStats,
    PoolLiquidity, ServiceTarget, SubmitAccepted, SwapQuote, TokenPrice, Transaction, Wallet,
    honest_gap_reason,
};
pub use config::{CliConfig, ConfigManager, ConfigSource, Network};
pub use consensus::{BlockHeader, ConsensusAlgorithm, ConsensusEngine, ConsensusParameters};
pub use crypto::{KeyManager, TransactionSigningData, WrfKeyPair};
pub use db::{
    BlockCacheEntry, DbClient, NFTCacheEntry, TransactionCacheEntry, WalletCacheEntry,
    WeatherQueueEntry,
};
pub use errors::CliErrorCode;
pub use security::{
    AntiDDoSOptions, CheckpointBlock, SecurityService, ValidationOptions, VerificationResult,
};
pub use websocket::{WebSocketClient, WebSocketConfig, WebSocketMessage};

/// Wrfcoin CLI version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Constants for the Wrfcoin CLI
pub mod constants {
    /// Existing ceiling for the complete pretty-printed station artifact.
    /// Mirrors launch_api/state.rs without a native-chain runtime dependency.
    /// Includes metadata, whitespace and the final newline.
    pub const MAX_STATION_ARTIFACT_BYTES: usize = 64 * 1024;
    /// Conservative phone-barometer/unverified batch recommendation on 777/testnet.
    /// Calibration uses complete signed transactions; this is not a protocol row cap.
    pub const CALIBRATED_MOBILE_STATION_BATCH_ROWS: usize = 21;

    /// Default API port
    pub const DEFAULT_API_PORT: u16 = 3000;
    /// Default WebSocket port
    pub const DEFAULT_WS_PORT: u16 = 5001;
    /// Default transaction fee in base units (0.001 WRF = 100_000 units)
    pub const DEFAULT_TX_FEE: u64 = 100_000;
    /// Maximum cache age in seconds (24 hours)
    pub const MAX_CACHE_AGE_SECONDS: u64 = 86400;
    /// Default mining difficulty
    pub const DEFAULT_MINING_DIFFICULTY: u32 = 4;
    /// Default checkpoint block interval
    pub const DEFAULT_CHECKPOINT_INTERVAL: u64 = 2016;
    /// Maximum reorganization depth
    pub const MAX_REORG_DEPTH: u64 = 100;
    /// Target block time in seconds
    pub const TARGET_BLOCK_TIME: u64 = 15;
    /// Default anti-DDoS request rate limit per minute
    pub const DEFAULT_REQUEST_RATE_LIMIT: u32 = 120;
}

/// Result type with anyhow::Error
pub type Result<T> = std::result::Result<T, anyhow::Error>;

#[cfg(test)]
mod integration_target_registration {
    /// #2888: a new `tests/*.rs` target must be registered in
    /// `scripts/integration_test_ratchet.json` or nothing in CI ever runs it.
    /// This fails at `cargo test -p <this crate>`, i.e. at the point of
    /// addition, not in a later PR's ratchet.
    #[test]
    fn every_integration_target_under_tests_is_in_the_ratchet_inventory() {
        wrfcoin_shared_protocol::integration_inventory::assert_registered(
            env!("CARGO_MANIFEST_DIR"),
            env!("CARGO_PKG_NAME"),
        );
    }
}
