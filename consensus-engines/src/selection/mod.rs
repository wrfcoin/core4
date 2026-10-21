//! Validator selection mechanisms for consensus
//!
//! This module provides VRF-based validator selection and probabilistic
//! finality tracking with BFT checkpoints.

pub mod bft_finality;
pub mod finality;
pub mod validator_set_source;
pub mod vrf;
