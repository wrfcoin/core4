//! Wrfcoin Core Testing Framework
//! 
//! This module provides comprehensive testing utilities for the Wrfcoin blockchain platform.
//! It includes unit test harnesses, integration test frameworks, and specialized testing
//! utilities for climate data, cross-chain operations, and consensus mechanisms.

pub mod unit_tests;
pub mod integration_tests;
pub mod performance_tests;
pub mod security_tests;
pub mod climate_data_tests;
pub mod cross_chain_tests;
pub mod consensus_tests;

// Test utilities and common functions
pub mod test_utils;
pub mod mock_objects;
pub mod test_fixtures;

// Re-export commonly used testing components
pub use unit_tests::*;
pub use test_utils::*;
pub use mock_objects::*;

/// Main test configuration for the Wrfcoin testing framework
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub enable_logging: bool,
    pub test_data_path: String,
    pub network_mode: NetworkMode,
    pub database_url: Option<String>,
}

#[derive(Debug, Clone)]
pub enum NetworkMode {
    Local,
    Testnet,
    Mainnet,
    Simulation,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            enable_logging: true,
            test_data_path: "tests/data".to_string(),
            network_mode: NetworkMode::Local,
            database_url: std::env::var("DATABASE_URL").ok(),
        }
    }
}

/// Initialize the testing framework with default configuration
pub fn init_test_framework() -> TestConfig {
    init_test_framework_with_config(TestConfig::default())
}

/// Initialize the testing framework with custom configuration
pub fn init_test_framework_with_config(config: TestConfig) -> TestConfig {
    if config.enable_logging {
        env_logger::init();
    }
    
    // Ensure test data directory exists
    std::fs::create_dir_all(&config.test_data_path).unwrap_or_else(|e| {
        panic!("Failed to create test data directory: {}", e);
    });
    
    config
}