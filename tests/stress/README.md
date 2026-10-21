# tests/stress — Manual Load Test Tools

## Status: Manual/Integration (not run by `cargo test`)

These Rust files are HTTP load-test drivers that must be run against a live
`wrfcoin-node` process.  They contain no `#[test]` functions and are therefore
**not executed** during `cargo test --workspace`.

A compilation shim at `native-chain/tests/stress_tests.rs` wires these files
into `cargo check` so that type errors are caught early, but the shim runs 0
tests.

## Contents

| File | Purpose |
|------|---------|
| `stress_test_framework.rs` | Core `StressTestRunner` — concurrent HTTP client, stats collection |
| `transaction_flooding_test.rs` | High-volume transaction submission |
| `network_partition_test.rs` | Split-brain scenario simulation |
| `block_size_test.rs` | Block-fill profiling under sustained load |

## How to run manually

```bash
# Ensure a node is running
cargo run -p wrfcoin-native-chain --bin launch_api

# Build a small binary that calls the runners:
#   (or use the CJS helpers: tests/stress/test-burst-transfers.cjs,
#    tests/stress/test-zk-batch.cjs, tests/stress/debug-sig.cjs)
WRFCOIN_TEST_API_URL=http://localhost:8556 node tests/stress/test-burst-transfers.cjs
```

## Compile check only

```bash
cargo test -p wrfcoin-native-chain --test stress_tests --no-run
```
