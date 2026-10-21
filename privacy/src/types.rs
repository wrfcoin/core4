//! Shared types used across privacy modules.

/// Re-exported from `wrf-primitives` — single source of truth for `PrivacyLevel`.
pub use wrf_primitives::PrivacyLevel;

use serde::{Deserialize, Serialize};

/// Configuration knobs for the privacy subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettings {
    /// Default privacy level for new transactions.
    pub default_level: PrivacyLevel,
    /// Enable stealth addresses by default.
    pub enable_stealth_addresses: bool,
    /// Enable confidential transactions by default.
    pub enable_confidential_transactions: bool,
    /// Enable ring signatures by default.
    pub enable_ring_signatures: bool,
    /// Minimum ring size for ring signatures.
    pub min_ring_size: usize,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            default_level: PrivacyLevel::Basic,
            enable_stealth_addresses: true,
            enable_confidential_transactions: false,
            enable_ring_signatures: false,
            min_ring_size: 11,
        }
    }
}
