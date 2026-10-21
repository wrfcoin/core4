#![warn(clippy::unwrap_used)]
// TODO(#606): shared-protocol carries pre-existing lint debt; keep -D warnings green
// while we migrate module-by-module to strict clippy compliance.
#![allow(
    dead_code,
    unexpected_cfgs,
    unused_imports,
    unused_mut,
    unused_variables,
    clippy::absurd_extreme_comparisons,
    clippy::disallowed_types,
    clippy::derivable_impls,
    clippy::excessive_nesting,
    clippy::enum_variant_names,
    clippy::large_enum_variant,
    clippy::manual_option_as_slice,
    clippy::needless_borrow,
    clippy::needless_borrows_for_generic_args,
    clippy::needless_return,
    clippy::new_without_default,
    clippy::only_used_in_recursion,
    clippy::redundant_closure,
    clippy::redundant_pattern_matching,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_lazy_evaluations,
    clippy::unnecessary_map_or,
    clippy::uninlined_format_args,
    clippy::useless_format,
    clippy::unwrap_used
)]

pub mod accounts;
pub mod blockchain;
pub mod bridge;
pub mod common;
pub mod config;
pub mod consensus;
pub mod contributions;
pub mod cross_chain;
pub mod crypto;
/// External-feed ingestion connectors. Behind the non-default `data-hub`
/// feature (core4#2316): it is the sole consumer of `reqwest` and `csv`, and
/// nothing in this workspace calls it, so shipping it in the default build put
/// an HTTP client in the consensus crate for no caller. `quick-xml` is NOT in
/// that set — it stays in the default dependency graph because `iso20022`
/// (a default, ungated module) parses and writes ISO 20022 XML with it, which
/// is exactly why `Cargo.toml` leaves `quick-xml` non-optional.
#[cfg(feature = "data-hub")]
pub mod data_hub;
pub mod governance;
pub mod identity;
pub mod invariants;
pub mod iso20022;
pub mod layer2;
pub mod mempool;
pub mod mesh;
pub mod monitoring;
pub mod nft;
pub mod oracle;
pub mod rewards;
pub mod security;
pub mod staking;
pub mod station_registry;
pub mod weather;

pub use wrfcoin_dataset_manifest as dataset_manifest;

/// Re-exported so every crate that depends on shared-protocol can run the
/// #2888 integration-target registration guard without a direct dependency on
/// `wrf-primitives`.
#[cfg(feature = "integration-guard")]
pub use wrf_primitives::integration_inventory;

#[cfg(test)]
mod integration_target_registration {
    /// #2888: a new `tests/*.rs` target must be registered in
    /// `scripts/integration_test_ratchet.json` or nothing in CI ever runs it.
    /// This fails at `cargo test -p wrfcoin-shared-protocol`, i.e. at the
    /// point of addition, not in a later PR's ratchet.
    #[test]
    fn every_integration_target_under_tests_is_in_the_ratchet_inventory() {
        wrf_primitives::integration_inventory::assert_registered(
            env!("CARGO_MANIFEST_DIR"),
            env!("CARGO_PKG_NAME"),
        );
    }
}

#[cfg(test)]
extern crate self as wrfcoin_shared_protocol;
