/// Stress-test shim — wires `tests/stress/` into the cargo compilation harness.
///
/// The modules under `tests/stress/` are standalone Rust files that are not
/// part of any workspace crate, so `cargo check --workspace` cannot detect
/// compilation drift without this shim.
///
/// Unlike `tests/security/`, the stress modules contain **no `#[test]` functions**.
/// They are async HTTP load-test drivers designed to run against a live node at
/// `WRFCOIN_TEST_API_URL` (default: `http://localhost:8556`).  They are
/// intentionally excluded from the `cargo test` run — see `tests/stress/README.md`
/// for how to run them manually.
///
/// This file exists solely so that `cargo check` and `cargo test --no-run` catch
/// compile errors in the stress codebase.
///
/// Run the compilation check:
///   cargo test -p wrfcoin-native-chain --test stress_tests --no-run
#[allow(dead_code, unused_imports, unused_variables, unused_must_use)]
#[path = "../../tests/stress/mod.rs"]
mod stress;

// An integration binary that reports "0 passed; 0 failed; 0 ignored; 0 filtered
// out" is byte-identical to one that passed everything, and it disappears
// entirely behind its siblings' totals in any package-wide count (#2911 found
// this target that way; #2861 found the same shape in
// shared-protocol/tests/yubikey_signer.rs). This file's emptiness is deliberate
// and documented above — but "deliberate" and "declared" are different claims,
// and only the second one is countable. The sentinel makes the absence show up
// as `1 ignored` instead of as silence, which is #2748's principle applied to a
// compile-only shim.
#[test]
#[ignore = "this target is a COMPILE-ONLY shim: tests/stress/ holds async HTTP load drivers with no #[test] functions, which must run against a live node. See tests/stress/README.md. The sentinel exists so the emptiness is countable rather than indistinguishable from a passing binary."]
fn this_target_is_a_compile_only_shim_and_runs_no_tests() {}
