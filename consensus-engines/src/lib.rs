//! Consensus engines for the WRFCoin blockchain
//!
//! This crate provides a modular consensus architecture supporting multiple
//! consensus algorithms including Proof-of-Work, Proof-of-Stake, and
//! Proof-of-Publishing for climate data validation.
//!
//! # Architecture
//!
//! The crate is organized around a core `ConsensusEngine` trait that all
//! consensus algorithm implementations must satisfy. A `ConsensusEngineManager`
//! provides runtime management of multiple algorithms with hot-swapping support,
//! and a `ConsensusFactory` offers convenient construction.

pub mod engine;
pub mod equivocation;
pub mod factory;
pub mod traits;
pub mod types;
pub mod validator;

pub mod algorithms;
pub mod coordination;
pub mod security;
pub mod selection;
pub mod economics;

// Re-export commonly used items
pub use engine::{ConsensusEngineBuilder, ConsensusEngineManager, TypedConsensusManager};
pub use algorithms::pop_adapter::PopConsensusAdapter;
pub use factory::{ConsensusBuilder, ConsensusFactory, create_consensus};
pub use traits::{ConsensusEngine, DynamicConsensus, NetworkStats};
pub use types::{
    ClimateDataType, ClimatePublication, ConsensusConfig, ConsensusError, ConsensusResult,
    ConsensusType, MiningStats, PublicationScore, ScoreWeights, ValidatorInfo, VerificationProof,
    VerificationProofType,
};
pub use validator::{
    ConsensusParams, PerformanceMetrics, SlashingEvent, SlashingSeverity, Validator,
    ValidatorManager, ValidatorStatus, ViolationType,
};
pub use selection::bft_finality::{
    BftFinalityError, BftFinalityGadget, FinalityCertificate, FinalityEvent, FinalityValidator,
    FinalityVote,
};

#[cfg(test)]
mod wire_layout_fixtures;

#[cfg(test)]
mod integration_target_registration {
    /// #2888: a new `tests/*.rs` target must be registered in
    /// `scripts/integration_test_ratchet.json` or nothing in CI ever runs it.
    /// This fails at `cargo test -p <this crate>`, i.e. at the point of
    /// addition, not in a later PR's ratchet.
    #[test]
    fn every_integration_target_under_tests_is_in_the_ratchet_inventory() {
        wrfcoin_shared_protocol::integration_inventory::assert_registered(
            env!("CARGO_MANIFEST_DIR"),
            env!("CARGO_PKG_NAME"),
        );
    }
}
