//! Test utilities for Wrfcoin testing framework
//! 
//! This module provides common testing utilities, helpers, and assertion macros
//! that are used across the entire test suite.

use std::time::{Duration, Instant};
use std::collections::HashMap;

/// Common test result type
pub type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Test timing utilities
pub struct TestTimer {
    start: Instant,
    name: String,
}

impl TestTimer {
    pub fn new(name: &str) -> Self {
        Self {
            start: Instant::now(),
            name: name.to_string(),
        }
    }
    
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    
    pub fn assert_within(&self, max_duration: Duration) {
        let elapsed = self.elapsed();
        assert!(
            elapsed <= max_duration,
            "Test '{}' took {:?}, expected <= {:?}",
            self.name, elapsed, max_duration
        );
    }
}

/// Test data generation utilities
pub struct TestDataGenerator {
    seed: u64,
}

impl TestDataGenerator {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }
    
    pub fn generate_test_address(&mut self) -> String {
        self.seed += 1;
        format!("test_address_{:016x}", self.seed)
    }
    
    pub fn generate_test_hash(&mut self) -> String {
        self.seed += 1;
        format!("test_hash_{:064x}", self.seed)
    }
    
    pub fn generate_climate_data(&mut self) -> ClimateTestData {
        self.seed += 1;
        ClimateTestData {
            temperature: 20.0 + (self.seed % 40) as f64,
            humidity: 30.0 + (self.seed % 40) as f64,
            pressure: 1000.0 + (self.seed % 100) as f64,
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClimateTestData {
    pub temperature: f64,
    pub humidity: f64,
    pub pressure: f64,
    pub timestamp: u64,
}

/// Environment setup utilities
pub fn setup_test_environment() -> TestResult<()> {
    std::env::set_var("RUST_LOG", "debug");
    std::env::set_var("TEST_MODE", "true");
    Ok(())
}

pub fn cleanup_test_environment() -> TestResult<()> {
    std::env::remove_var("TEST_MODE");
    Ok(())
}

/// Assertion helpers for blockchain-specific testing
pub fn assert_valid_hash(hash: &str) {
    assert!(!hash.is_empty(), "Hash should not be empty");
    assert!(hash.len() >= 32, "Hash should be at least 32 characters");
}

pub fn assert_valid_address(address: &str) {
    assert!(!address.is_empty(), "Address should not be empty");
    assert!(address.len() >= 20, "Address should be at least 20 characters");
}

/// Test configuration management
#[derive(Debug, Clone)]
pub struct TestEnvironment {
    pub vars: HashMap<String, String>,
}

impl TestEnvironment {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }
    
    pub fn set_var(&mut self, key: &str, value: &str) {
        self.vars.insert(key.to_string(), value.to_string());
        std::env::set_var(key, value);
    }
    
    pub fn get_var(&self, key: &str) -> Option<&String> {
        self.vars.get(key)
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        for key in self.vars.keys() {
            std::env::remove_var(key);
        }
    }
}

/// Macros for common test patterns
#[macro_export]
macro_rules! test_with_timeout {
    ($timeout:expr, $test_fn:expr) => {
        {
            let timer = $crate::test_utils::TestTimer::new("timeout_test");
            let result = $test_fn;
            timer.assert_within($timeout);
            result
        }
    };
}

#[macro_export]
macro_rules! assert_climate_data_valid {
    ($data:expr) => {
        assert!($data.temperature >= -100.0 && $data.temperature <= 100.0, 
               "Temperature out of valid range: {}", $data.temperature);
        assert!($data.humidity >= 0.0 && $data.humidity <= 100.0, 
               "Humidity out of valid range: {}", $data.humidity);
        assert!($data.pressure >= 800.0 && $data.pressure <= 1200.0, 
               "Pressure out of valid range: {}", $data.pressure);
    };
}