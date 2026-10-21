#![cfg(feature = "full-mining")]
//! Mining service implementation
//!
//! This module provides a concrete implementation of the MiningService trait
//! for performing mining operations.
//!
//! Migrated from core4_src/mining/service.rs (issue #397).

use anyhow::{Result, anyhow};
use log::{debug, info};
use std::sync::{Arc, Mutex};

use crate::mining::hardware_detection::{
    HardwareCapabilities, HardwareConfig, HardwareManager,
};

// TODO(#960): Re-add when core4_src/blockchain/chain.rs is migrated to a workspace crate.
// use crate::blockchain::chain::Blockchain;
// TODO(#960): Re-add when the storage layer is migrated to a workspace crate.
// use crate::storage::database::DatabaseImpl;
// use tokio::sync::RwLock;
// use tokio::task::JoinHandle;
// use wrfcoin_shared_protocol::blockchain::block::Block;
// TODO(#960): Re-add when mining sub-modules are migrated.
// use crate::mining::setup::{OneClickMiningSetup, MiningSetupConfig, MiningSetupStatus};
// use crate::mining::pools::{MiningPoolManager, PoolMember, PoolSpecialization};
// use crate::mining::rewards::{RewardDistributor, RewardDistributionConfig};

/// Mining status
#[derive(Debug, Clone, Default)]
pub struct MiningStatus {
    pub is_mining: bool,
    pub hash_rate: u64,
    pub reward_address: Option<String>,
    pub hardware_config: Option<HardwareConfig>,
    pub pool_info: Option<MiningPoolInfo>,
    pub blocks_mined: u64,
    pub rewards_earned: u64,
}

/// Mining pool info for status reporting
#[derive(Debug, Clone)]
pub struct MiningPoolInfo {
    pub id: String,
    pub name: String,
    pub pool_hash_rate: u64,
    pub miner_count: u64,
    pub miner_share: f64,
    pub pending_rewards: u64,
}

/// Mining configuration
#[derive(Debug, Clone, Default)]
pub struct MiningConfig {
    pub threads: u32,
    pub intensity: f64,
}

/// Mining pool configuration
#[derive(Debug, Clone)]
pub struct MiningPoolConfig {
    pub name: String,
    pub fee_percentage: u8,
}

/// Mining result
#[derive(Debug, Clone)]
pub struct MiningResult {
    pub block_hash: String,
    pub height: u64,
    pub reward: u64,
}

/// Mining service trait
pub trait MiningService {
    fn start_one_click_mining(&self, reward_address: &str) -> Result<()>;
    fn start_mining_with_config(
        &self,
        reward_address: &str,
        hw_config: &HardwareConfig,
    ) -> Result<()>;
    fn stop_mining(&self) -> Result<()>;
    fn get_mining_status(&self) -> Result<MiningStatus>;
    fn join_pool(&self, pool_id: &str, miner_address: &str) -> Result<()>;
    fn leave_pool(&self) -> Result<()>;
    fn create_pool(&self, config: MiningPoolConfig) -> Result<String>;
}

/// Wrfcoin mining service implementation
pub struct WrfcoinMiningService {
    /// Hardware manager
    hardware_manager: Arc<Mutex<HardwareManager>>,

    /// Current mining status
    status: Arc<Mutex<MiningStatus>>,

    /// Mining configuration (TODO(#960): used once config-hot-reload path is wired)
    #[allow(dead_code)]
    config: Arc<Mutex<MiningConfig>>,

    /// Currently active pool
    active_pool_id: Arc<Mutex<Option<String>>>,
}

impl WrfcoinMiningService {
    /// Create a new Wrfcoin mining service
    pub fn new() -> Result<Self> {
        // Create hardware manager
        let hardware_manager = Arc::new(Mutex::new(HardwareManager::new()));

        // Create mining config
        let config = Arc::new(Mutex::new(MiningConfig::default()));

        // Create initial mining status
        let status = Arc::new(Mutex::new(MiningStatus {
            is_mining: false,
            hash_rate: 0,
            reward_address: None,
            hardware_config: None,
            pool_info: None,
            blocks_mined: 0,
            rewards_earned: 0,
        }));

        // Create active pool ID
        let active_pool_id = Arc::new(Mutex::new(None));

        Ok(Self {
            hardware_manager,
            status,
            config,
            active_pool_id,
        })
    }

    /// Get hardware capabilities with lazy initialization
    pub fn get_hardware_capabilities(&self) -> Result<HardwareCapabilities> {
        let mut hardware_manager = self
            .hardware_manager
            .lock()
            .map_err(|_| anyhow!("hardware_manager mutex poisoned"))?;

        // Lazy initialization - only perform expensive hardware detection when needed
        if !hardware_manager.is_initialized() {
            debug!("Initializing hardware detection on first capability request");
            hardware_manager.initialize()?;
        }

        hardware_manager.get_capabilities()
    }

    /// Get optimal hardware configuration with lazy initialization
    pub fn get_optimal_config(&self) -> Result<HardwareConfig> {
        let mut hardware_manager = self
            .hardware_manager
            .lock()
            .map_err(|_| anyhow!("hardware_manager mutex poisoned"))?;

        // Lazy initialization - only perform expensive hardware detection when needed
        if !hardware_manager.is_initialized() {
            debug!("Initializing hardware detection on first config request");
            hardware_manager.initialize()?;
        }

        hardware_manager.get_optimal_config()
    }
}

impl MiningService for WrfcoinMiningService {
    /// Start mining with automatic hardware detection
    fn start_one_click_mining(&self, reward_address: &str) -> Result<()> {
        // Get hardware capabilities
        let _hw_capabilities = self.get_hardware_capabilities()?;

        // Get optimal configuration
        let hw_config = self.get_optimal_config()?;

        // Update mining status
        {
            let mut status = self
                .status
                .lock()
                .map_err(|_| anyhow!("mining status mutex poisoned"))?;
            status.is_mining = true;
            status.reward_address = Some(reward_address.to_string());
            status.hardware_config = Some(hw_config.clone());
        }

        // Log the start of mining
        info!("Starting one-click mining for address: {}", reward_address);
        info!("Using CPU threads: {}", hw_config.cpu_threads);
        if !hw_config.selected_gpus.is_empty() {
            info!("Using {} GPUs", hw_config.selected_gpus.len());
        }

        // In a real implementation, this would launch mining threads
        // For now, we'll just update the status

        Ok(())
    }

    /// Start mining with specific hardware configuration
    fn start_mining_with_config(
        &self,
        reward_address: &str,
        hw_config: &HardwareConfig,
    ) -> Result<()> {
        // Update mining status
        {
            let mut status = self
                .status
                .lock()
                .map_err(|_| anyhow!("mining status mutex poisoned"))?;
            status.is_mining = true;
            status.reward_address = Some(reward_address.to_string());
            status.hardware_config = Some(hw_config.clone());
        }

        // Log the start of mining
        info!(
            "Starting mining with custom config for address: {}",
            reward_address
        );
        info!("Using CPU threads: {}", hw_config.cpu_threads);
        if !hw_config.selected_gpus.is_empty() {
            info!("Using {} GPUs", hw_config.selected_gpus.len());
        }

        // In a real implementation, this would launch mining threads
        // For now, we'll just update the status

        Ok(())
    }

    /// Stop mining
    fn stop_mining(&self) -> Result<()> {
        // Update mining status
        {
            let mut status = self
                .status
                .lock()
                .map_err(|_| anyhow!("mining status mutex poisoned"))?;
            status.is_mining = false;
        }

        // Log the stop of mining
        info!("Stopping mining");

        // In a real implementation, this would stop mining threads
        // For now, we'll just update the status

        Ok(())
    }

    /// Get current mining status
    fn get_mining_status(&self) -> Result<MiningStatus> {
        let status = self
            .status
            .lock()
            .map_err(|_| anyhow!("mining status mutex poisoned"))?;
        Ok(status.clone())
    }

    /// Join a mining pool
    fn join_pool(&self, pool_id: &str, miner_address: &str) -> Result<()> {
        // Set active pool
        {
            let mut active_pool = self
                .active_pool_id
                .lock()
                .map_err(|_| anyhow!("active_pool_id mutex poisoned"))?;
            *active_pool = Some(pool_id.to_string());
        }

        // Update mining status with pool info
        {
            let mut status = self
                .status
                .lock()
                .map_err(|_| anyhow!("mining status mutex poisoned"))?;

            // TODO: Fetch actual pool info
            status.pool_info = Some(MiningPoolInfo {
                id: pool_id.to_string(),
                name: "Unknown Pool".to_string(), // This would be fetched from the pool
                pool_hash_rate: 0,
                miner_count: 0,
                miner_share: 0.0,
                pending_rewards: 0,
            });
        }

        // Log joining the pool
        info!(
            "Joined mining pool {} with address {}",
            pool_id, miner_address
        );

        Ok(())
    }

    /// Leave current mining pool
    fn leave_pool(&self) -> Result<()> {
        // Get current pool ID
        let pool_id = {
            let active_pool = self
                .active_pool_id
                .lock()
                .map_err(|_| anyhow!("active_pool_id mutex poisoned"))?;
            active_pool.clone()
        };

        if let Some(pool_id) = pool_id {
            // Clear active pool
            {
                let mut active_pool = self
                    .active_pool_id
                    .lock()
                    .map_err(|_| anyhow!("active_pool_id mutex poisoned"))?;
                *active_pool = None;
            }

            // Update mining status
            {
                let mut status = self
                    .status
                    .lock()
                    .map_err(|_| anyhow!("mining status mutex poisoned"))?;
                status.pool_info = None;
            }

            // Log leaving the pool
            info!("Left mining pool {}", pool_id);
        } else {
            return Err(anyhow!("Not currently in a mining pool"));
        }

        Ok(())
    }

    /// Create a new mining pool
    fn create_pool(&self, config: MiningPoolConfig) -> Result<String> {
        // Delegate to pool manager
        // In a real implementation, this would call the pool manager
        // For now, we'll return a placeholder pool ID

        let pool_id = format!("pool-{}", rand::random::<u32>());

        // Log creating the pool
        info!("Created mining pool {} with name {}", pool_id, config.name);

        Ok(pool_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Happy path: a newly created service reports is_mining=false and all
    /// counters at zero.
    #[test]
    fn new_service_starts_not_mining() {
        let service = WrfcoinMiningService::new().expect("service creation should succeed");
        let status = service.get_mining_status().expect("status must be readable on new service");
        assert!(!status.is_mining, "new service must not be mining");
        assert_eq!(status.blocks_mined, 0, "blocks_mined must start at 0");
        assert_eq!(status.rewards_earned, 0, "rewards_earned must start at 0");
        assert!(status.reward_address.is_none(), "reward_address must be None initially");
        assert!(status.pool_info.is_none(), "pool_info must be None initially");
    }

    /// Happy path: start_mining_with_config sets is_mining=true with the given
    /// reward address; stop_mining returns it to false.
    #[test]
    fn start_stop_cycle_toggles_is_mining() {
        let service = WrfcoinMiningService::new().expect("service creation should succeed");
        let hw_config = HardwareConfig::default();

        service
            .start_mining_with_config("wrfcoin1abc", &hw_config)
            .expect("start_mining_with_config must succeed");

        let status = service.get_mining_status().expect("status readable after start");
        assert!(status.is_mining, "is_mining must be true after start");
        assert_eq!(
            status.reward_address.as_deref(),
            Some("wrfcoin1abc"),
            "reward_address must match the address passed to start"
        );

        service.stop_mining().expect("stop_mining must succeed");

        let status = service.get_mining_status().expect("status readable after stop");
        assert!(!status.is_mining, "is_mining must be false after stop");
    }

    /// Error path: leave_pool when no pool is active must return Err.
    #[test]
    fn leave_pool_without_joining_returns_err() {
        let service = WrfcoinMiningService::new().expect("service creation should succeed");
        let result = service.leave_pool();
        assert!(
            result.is_err(),
            "leave_pool must return Err when no pool is active"
        );
    }
}
