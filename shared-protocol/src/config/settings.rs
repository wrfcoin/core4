use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Configuration settings for the Wrfcoin node
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// Chain ID for this blockchain network
    pub chain_id: String,

    /// Network name (mainnet, testnet, etc.)
    pub network: String,

    /// Database path for blockchain storage
    pub database_path: String,

    /// Peer-to-peer network port
    pub p2p_port: u16,

    /// API server host
    pub api_host: String,

    /// API server port
    pub api_port: u16,

    /// Initial mining difficulty
    pub mining_difficulty: u32,

    /// Target block time in seconds
    pub block_time_target_seconds: u64,

    /// Initial block reward amount
    pub initial_block_reward: u64,

    /// Number of blocks between reward halvings
    pub reward_halving_interval: u64,

    /// List of bootstrap peers to connect to
    pub bootstrap_peers: Vec<String>,

    /// Genesis address to receive initial coins
    pub genesis_address: String,

    /// Genesis block reward
    pub genesis_reward: u64,

    /// Maximum block size in bytes
    pub max_block_size: usize,

    /// Maximum transaction size in bytes
    pub max_transaction_size: usize,

    /// Maximum number of transactions per block
    pub max_transactions_per_block: usize,

    /// List of trusted validator addresses
    pub trusted_validators: Vec<String>,

    /// List of cross-chain validator addresses
    pub cross_chain_validators: Option<Vec<String>>,

    /// Minimum required signatures for cross-chain transactions
    pub cross_chain_min_signatures: Option<usize>,

    /// Enable cross-chain bridging
    pub enable_cross_chain: Option<bool>,

    /// Maximum age of transactions in the mempool (seconds).
    /// Transactions older than this are evicted during cleanup.
    /// Default: 14400 (4 hours).
    #[serde(default = "default_mempool_max_age_secs")]
    pub mempool_max_age_secs: u64,

    /// Maximum mempool size in bytes.
    /// Default: 52428800 (50 MB).
    #[serde(default = "default_mempool_max_size_bytes")]
    pub mempool_max_size_bytes: usize,

    /// Minimum mempool fee rate for admission, in the same units as the
    /// mempool's `fee_rate` sort key (#2039). A transaction whose fee_rate is
    /// below this floor is rejected at admission. Default: 0 (floor disabled) —
    /// deliberately off on testnet so faucet-funded zero-fee earn transactions
    /// are still admitted; raise it operationally once faucet output is coupled
    /// to a fee.
    #[serde(default = "default_mempool_min_fee_rate")]
    pub mempool_min_fee_rate: u64,
    /// General verification-work admission price, never a consensus debit.
    /// Zero preserves existing defaults; applies uniformly to every tx kind.
    #[serde(default)]
    pub mempool_verify_fee_per_unit: u64,

    /// Maximum number of unconfirmed transactions a single sender may hold in
    /// the mempool (#2039 anti-spam). Keyed on the (spoofable) `sender_address`,
    /// so it bounds naive per-key floods; authenticated-sender capping is a
    /// follow-up. Default: 1024 (generous — well above any legitimate
    /// single-account concurrency, including the collector/faucet).
    #[serde(default = "default_mempool_max_txs_per_sender")]
    pub mempool_max_txs_per_sender: usize,

    /// DA challenge producer interval in seconds. Set to 0 to disable.
    #[serde(default = "default_da_challenge_interval_secs")]
    pub da_challenge_interval_secs: u64,

    /// Number of peers to target per DA challenge round. Set to 0 to disable.
    #[serde(default = "default_da_challenge_peer_count")]
    pub da_challenge_peer_count: usize,

    /// Timeout window in seconds before a DA challenge is considered failed.
    #[serde(default = "default_da_challenge_timeout_secs")]
    pub da_challenge_timeout_secs: u64,
}

impl Config {
    /// Loads the configuration from a file or creates a default one
    pub fn load(config_path: Option<&str>) -> Result<Self> {
        // Try to load from file if provided
        if let Some(path) = config_path {
            if Path::new(path).exists() {
                let config_str = fs::read_to_string(path)?;
                let config: Config = serde_json::from_str(&config_str)?;
                info!("Loaded configuration from {path}");
                return Ok(config);
            }
        }

        // No valid config file found, use defaults
        info!("Using default configuration");
        Ok(Self::default())
    }

    /// Saves the configuration to a file
    pub fn save(&self, path: &str) -> Result<()> {
        let config_str = serde_json::to_string_pretty(self)?;
        fs::write(path, config_str)?;
        info!("Saved configuration to {path}");
        Ok(())
    }
}

fn default_mempool_max_age_secs() -> u64 {
    4 * 3600 // 4 hours
}

fn default_mempool_max_size_bytes() -> usize {
    50 * 1024 * 1024 // 50 MB
}

fn default_mempool_min_fee_rate() -> u64 {
    0 // floor disabled by default (see field doc — protects testnet faucet)
}

fn default_mempool_max_txs_per_sender() -> usize {
    1024
}

fn default_da_challenge_interval_secs() -> u64 {
    60
}

fn default_da_challenge_peer_count() -> usize {
    3
}

fn default_da_challenge_timeout_secs() -> u64 {
    30
}

impl Default for Config {
    fn default() -> Self {
        Self {
            chain_id: "wrfcoin-main".to_string(),
            network: "mainnet".to_string(),
            database_path: "data/wrfcoin".to_string(),
            p2p_port: 6333,
            api_host: "0.0.0.0".to_string(),
            api_port: 8000,
            mining_difficulty: 16,                // Moderate initial difficulty
            block_time_target_seconds: 600,       // 10 minutes, like Bitcoin
            initial_block_reward: 50_000_000_000, // 500 WRF (in smallest unit)
            reward_halving_interval: 210_000,     // ~4 years at 10-minute blocks
            bootstrap_peers: vec![
                "/ip4/bootstrap1.wrfcoin.example:6333".to_string(),
                "/ip4/bootstrap2.wrfcoin.example:6333".to_string(),
            ],
            genesis_address: "wrf7a6eb3c86f0e5c5cd1fed57ff64c6b8c9210032d".to_string(),
            genesis_reward: 50_000_000_000, // 500 WRF (in smallest unit)
            max_block_size: 1_000_000,      // 1MB
            max_transaction_size: 100_000,  // 100KB
            max_transactions_per_block: 5_000,
            trusted_validators: vec!["wrf7a6eb3c86f0e5c5cd1fed57ff64c6b8c9210032d".to_string()],
            cross_chain_validators: Some(vec![
                "wrf7a6eb3c86f0e5c5cd1fed57ff64c6b8c9210032d".to_string(),
            ]),
            cross_chain_min_signatures: Some(1),
            enable_cross_chain: Some(true),
            mempool_max_age_secs: default_mempool_max_age_secs(),
            mempool_max_size_bytes: default_mempool_max_size_bytes(),
            mempool_min_fee_rate: default_mempool_min_fee_rate(),
            mempool_verify_fee_per_unit: 0,
            mempool_max_txs_per_sender: default_mempool_max_txs_per_sender(),
            da_challenge_interval_secs: default_da_challenge_interval_secs(),
            da_challenge_peer_count: default_da_challenge_peer_count(),
            da_challenge_timeout_secs: default_da_challenge_timeout_secs(),
        }
    }
}
