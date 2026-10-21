/// Security attack test shim — wires `tests/security/` into the cargo test harness.
///
/// The modules under `tests/security/` are standalone Rust files that are not
/// part of any workspace crate, so `cargo test --workspace` cannot discover
/// them on its own.  This integration-test file uses `#[path]` to pull in the
/// top-level `mod.rs`, after which cargo compiles and runs every `#[test]` and
/// `#[tokio::test]` function declared inside the tree.
///
/// Pure-function tests (config defaults, struct construction, enum variants)
/// run unconditionally.  Live-node tests are `#[ignore]` and guarded by the
/// `WRFCOIN_SECURITY_TEST=1` environment variable so they are safe in CI.
///
/// Run all tests (ignoring live-node tests):
///   cargo test -p wrfcoin-native-chain --test security_attacks
///
/// Run including live-node tests (requires node at localhost:8556):
///   WRFCOIN_SECURITY_TEST=1 cargo test -p wrfcoin-native-chain --test security_attacks -- --include-ignored
#[path = "../../tests/security/mod.rs"]
mod security;
