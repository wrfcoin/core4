//! Coordination infrastructure for consensus engines
//!
//! This module provides the coordination layer that sits above individual consensus
//! algorithm implementations. It handles Byzantine fault tolerance, dynamic engine
//! switching, adaptive fault tolerance, sharding, hard forks, gossip protocols,
//! proposer-builder separation, and advanced voting/governance mechanisms.
//!
//! ## Module Dependency Order
//!
//! The modules have the following dependency relationships:
//!
//! 1. `byzantine` - standalone BFT types and detection
//! 2. `sharding` - standalone sharding/beacon chain infrastructure
//! 3. `hard_fork` - standalone fork coordination
//! 4. `gossip` - standalone gossip protocol
//! 5. `proposer_builder_separation` - standalone PBS/MEV-Boost
//! 6. `dynamic_engine` - depends on crate-level types/traits/engine
//! 7. `byzantine_coordinator` - depends on `dynamic_engine` (TransitionResult/TransitionTrigger)
//! 8. `adaptive_fault_tolerance` - depends on `dynamic_engine` + `byzantine_coordinator`
//! 9. `consensus_builder` - depends on `byzantine_coordinator` (VoteChoice)

pub mod adaptive_fault_tolerance;
pub mod byzantine;
pub mod byzantine_coordinator;
pub mod consensus_builder;
pub mod dynamic_engine;
pub mod gossip;
pub mod hard_fork;
pub mod proposer_builder_separation;
pub mod sharding;

/// Backward-compatible re-export of the partition policy types.
///
/// The module was relocated to
/// [`wrfcoin_shared_protocol::consensus::partition_policy`] so that the
/// mempool and other lower-level modules can gate on [`PartitionMode`]
/// without a circular dependency. Existing callers that previously
/// imported `consensus_engines::coordination::partition_policy` continue
/// to work via this re-export.
pub mod partition_policy {
    pub use wrfcoin_shared_protocol::consensus::partition_policy::{
        classify_partition, mode_for_status, partition_mode, PartitionMode,
        PartitionQuorumStatus,
    };
}
