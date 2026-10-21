//! Economic models and fee markets for consensus
//!
//! This module provides EIP-1559 dynamic fee market implementation and
//! smart contract integration types for the consensus layer.

pub mod eip1559;
/// Bounded Kani proofs (+ runnable equivalents) over economic arithmetic (#2108).
pub mod proofs;
pub mod smart_contract;
// #2143: verification-work pricing lives in `shared-protocol` because the
// sites it gates — `ProofOfPublishing::create_next_block` (producer) and
// `verify_block` (applier/replay) — are there, and `shared-protocol` cannot
// depend on this crate (the dependency runs the other way). Re-exported here
// so the economics module remains the place to look for it.
pub use wrfcoin_shared_protocol::consensus::verify_cost;
