//! Database utilities for the native-chain storage layer.
//!
//! This module is a home for cross-cutting database concerns that live above
//! the raw redb API but below the domain-specific [`crate::storage`] module:
//!
//! - [`migrations`] — versioned schema migration framework

pub mod backup;
pub mod migrations;
