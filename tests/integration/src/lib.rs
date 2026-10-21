//! Library surface for the `test_server` integration crate.
//!
//! Currently exposes the deterministic-replay primitive for the determinism-assurance
//! epic (#2102). Kept `pub` so the CI determinism matrix (#2104) and the DA/DePIN epics
//! (#2183/#2204) can reuse [`determinism::replay_fixture`] instead of duplicating it.

pub mod determinism;
