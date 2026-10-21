//! Stress Testing Module for Wrfcoin
//!
//! This module contains various stress testing tools and frameworks
//! for the Wrfcoin blockchain, including transaction flooding tests,
//! network partition tests, and block size stress tests.

pub mod stress_test_framework;
pub mod transaction_flooding_test;
pub mod network_partition_test;
pub mod block_size_test;

// Re-export commonly used items
pub use stress_test_framework::{StressTestConfig, StressTestRunner, TransactionType};
pub use transaction_flooding_test::{
    run_transaction_flooding_test, run_burst_transaction_test, run_sustained_load_test
};
pub use network_partition_test::{
    run_network_partition_test, run_comprehensive_partition_tests
};
pub use block_size_test::run_block_size_test;