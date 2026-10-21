//! Security Testing Module for Wrfcoin
//!
//! This module contains various security testing tools and frameworks
//! for the Wrfcoin blockchain, including vulnerability assessments,
//! double-spending tests, and 51% attack simulations.

pub mod security_framework;
pub mod double_spend_test;
pub mod attack_51_percent;
pub mod fuzz_testing;

// NOTE: the security test framework's public items (SecurityTestRunner,
// FuzzTestRunner, run_security_audit, run_fuzz_test, run_targeted_fuzz_test,
// run_double_spend_test, run_51_percent_attack, etc.) live in the submodules
// above and are exercised directly by the `#[test]`/`#[tokio::test]` functions
// and env-gated live-node harnesses there. They were previously re-exported
// here, but nothing consumes the re-exports in the `security_attacks` test
// binary, so the unused `pub use` block was removed (the items themselves are
// retained, marked `#[allow(dead_code)]` where not yet wired to a caller).