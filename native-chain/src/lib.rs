pub mod accounts;
pub mod db;
pub mod blockchain;
pub mod bridge;
pub mod common;
pub mod config;
pub mod consensus;
pub mod contributions;
pub mod cross_chain;
pub mod crypto;
/// Re-export of shared-protocol's external-feed connectors. Gated to match
/// (core4#2316) — the module is a bare `pub use` and cannot outlive its source.
#[cfg(feature = "data-hub")]
pub mod data_hub;
pub mod governance;
pub mod identity;
pub mod invariants;
pub mod iso20022;
pub mod layer2;
pub mod mempool;
pub mod mining;
pub mod monitoring;
pub mod network_failover;
pub mod nft;
pub mod oracle;
pub mod p2p_divergence;
pub mod security;
pub mod staking;
pub mod storage;
pub mod node_service_archive;
pub mod schedule_view;
pub mod upgrade;
pub mod upgrade_notice;
mod storage_encryption;
pub mod weather;
#[cfg(feature = "zk")]
pub mod zk_service;

#[cfg(test)]
mod parity;
#[cfg(test)]
mod parity_shared;
// #2302: the alias assertion and invariant-detection gate that `parity_shared`
// cannot provide, because both sides of its comparisons are the same item.
#[cfg(test)]
mod parity_shared_detection;
// #2330: an INDEPENDENT behavioural oracle for the staking parity row, so a bug
// shared by both sides of `parity_shared`'s native==shared comparison is caught,
// not masked.
#[cfg(test)]
mod parity_shared_oracle;

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
