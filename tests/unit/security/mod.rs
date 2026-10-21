//! Security Testing Module for Wrfcoin
//!
//! This module contains various security testing tools and frameworks
//! for the Wrfcoin blockchain, including vulnerability assessments,
//! double-spending tests, and 51% attack simulations.

pub mod security_framework;
pub mod double_spend_test;
pub mod attack_51_percent;
pub mod fuzz_testing;

// Re-export commonly used items
pub use security_framework::{
    SecurityTestConfig,
    SecurityTestRunner,
    Severity,
    Vulnerability,
    run_security_audit,
};

pub use double_spend_test::run_double_spend_test;
pub use attack_51_percent::run_51_percent_attack;
pub use fuzz_testing::{
    FuzzTestConfig,
    FuzzTestRunner,
    FuzzIssueSeverity,
    run_fuzz_test,
    run_targeted_fuzz_test,
};