//! Mining module — PoW contribution pools, hardware detection, threat detection,
//! reward distribution.
//!
//! Heavy dependencies (wallet, NFT, database) are gated behind the `full-mining`
//! feature flag until those modules are migrated to the workspace.
//! See TODO(#960) comments for remaining migration items.
//! Migrated from core4_src/mining/ (issue #397).

pub mod hardware_detection;
pub mod pool;
pub mod reward;
pub mod threat_detection;

#[cfg(feature = "full-mining")]
pub mod collaborative_pools;
#[cfg(feature = "full-mining")]
pub mod service;

pub use reward::{MiningContribution, ContributionType, RewardDistribution, RewardProofGenerator};
pub use pool::{
    MiningPool, PoolMember, MiningTask, PoolMetrics, MemberStatus, TaskStatus,
    MinerShare, ShareValidationResult, MinerDifficultyState, THREAT_SCORE_SUSPEND_THRESHOLD,
};
pub use threat_detection::{ThreatDetectionSystem, ThreatType, ThreatEvent, BehavioralProfile};
pub use hardware_detection::{HardwareProfile, CpuInfo, GpuInfo, MemoryInfo};
