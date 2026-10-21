pub mod error;
pub mod types;
pub mod docker;
pub mod pipeline;
pub mod models;
pub mod manifest;
pub mod registry;
pub mod verify;

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
