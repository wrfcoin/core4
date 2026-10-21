# tests/unit — Audit Notes (#1321)

## JavaScript/TypeScript unit tests (Jest)

The subdirectories (`api/`, `database/`, `governance/`, etc.) contain Jest unit
tests discovered automatically by `jest.config.cjs`.  They are the primary unit
test suite and are run via `npm run test:unit`.

## Rust files — LEGACY (not wired into cargo)

The four Rust files in this directory root are **stale copies** from the old
`blockchain-core` architecture:

| File | Issue |
|------|-------|
| `discovery_tests.rs` | Uses `crate::node::discovery::PeerDiscovery` — does not exist in core4 |
| `node_tests.rs` | Uses `crate::node::DefaultNodeOperator` — does not exist in core4 |
| `sync_tests.rs` | Uses `crate::node::sync::SyncManager` — does not exist in core4 |
| `hsm_integration_tests.rs` | Uses `crate::wallet::hsm::HsmManager` — does not exist in core4 |

These files have real `#[test]` functions (5 in `discovery_tests.rs`, 9+ in
`hsm_integration_tests.rs`, etc.) and represent meaningful test coverage, but
they cannot be wired up without rewriting the `use crate::...` paths to match
the current `wrfcoin-native-chain` / `wrfcoin-p2p` module structure.

**Recommended follow-up**: map each file to its equivalent current module and
rewrite the imports.  Do not delete without confirming that equivalent coverage
exists elsewhere.

Tracked in core4#1321.
