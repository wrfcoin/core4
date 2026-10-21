# Contributing to WRFCoin Core

Read the [Code of Conduct](CODE_OF_CONDUCT.md). Report vulnerabilities privately
using [SECURITY.md](SECURITY.md).

## Build and test

Install Rust with the toolchain pinned in [rust-toolchain.toml](rust-toolchain.toml),
OpenSSL development libraries, and the Protobuf compiler. Work in a development
checkout, away from a running testnet node.

```bash
git clone https://github.com/wrfcoin/core4.git
cd core4
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Format the files you change with `cargo fmt`. The workspace-wide formatting
check is advisory because main contains existing formatting debt.

Use targeted package tests while iterating; report the exact commands, results,
and any skipped coverage. Add regression tests for changed behavior, particularly
consensus, serialization, persistence, and security boundaries. Workflow definitions
in [.github/workflows](.github/workflows) describe CI coverage; a parked workflow
or an unexecuted job is not validation evidence.

## Branch and pull request flow

1. Search existing issues and pull requests before starting.
2. Fork the repository and create a focused branch from current `main`.
3. Make the change with relevant tests and documentation.
4. Run the checks above that cover the change and report limitations.
5. Open a pull request against `main` with a conventional title and `Refs #N`
   in the description. Include the problem, changed behavior, and test evidence.
6. Address review feedback. Maintainers handle admission and merging.

Never commit credentials, private keys, recovery phrases, personal locations, or
private operator details. Use synthetic fixtures for tests.

## Bincode Boundary Types Are Append-Only At Most (#2883)

Types serialized with bincode across a process boundary — the gossip mesh
(`WrfcoinMessage` payloads: `Block`, `Transaction`, `FinalityVote`,
`FinalityCertificate`, …), the `/wrfcoin/block-sync` protocol
(`BlockRequest`/`BlockResponse`), and persisted stores (`Block` archive,
`EquivocationEvidence`) — are encoded positionally: bincode writes shape and
order, not names. A field inserted, removed, or reordered mid-struct shifts
every later byte, and a mid-enum variant insertion renumbers every later
discriminant. Either is a format break for every peer and every persisted byte
(the #2727 hazard class; three such breaks shipped unnoticed in July–August
2026 — see #2884).

Golden byte fixtures (`wire_layout_fixtures` modules in `shared-protocol`,
`consensus-engines`, and `p2p`) pin every boundary type's canonical encoding
and will go RED on any layout change, telling you what to do: decide the
version explicitly — bump `WIRE_FORMAT_VERSION` for wire types (its own
tripwire then forces the topic/protocol identifier renames), or the storage
envelope version for persisted types (#2881's checkpoint pattern) — then
regenerate the fixture in the same PR. Enum variants and new `WrfcoinMessage`
kinds are append-only. Never regenerate a fixture without the version
decision: that silent regeneration is exactly the break the tripwire exists
to prevent.

## License

Contributions use the repository's existing [Apache-2.0 license](LICENSE).

## Public launch CI contract (2026-10-19 owner ruling)

Pull requests execute on GitHub-hosted runners. The proposed required contexts
are `public PR admission`, `schedule-lint`,
`detect-secrets baseline audit (merge tree)` and `verify-terminal-skip-guards`.
The owner must replace the old `repository-input integration admission` required
context when activating this contract; this change does not modify settings.

`public PR admission` requires successful execution of both workers: workspace
library clippy and unit tests, and workflow policy checks. The workspace-wide
format check remains advisory because of existing formatting debt. Existing
hosted path-selected gates continue running with their existing selection rules.
A skipped, cancelled or failed worker cannot satisfy the admission terminal.

The following properties move from PR-time admission to **post-merge or manual
CT102 evidence** under the zero-budget launch ruling:

- Full integration-target inventory execution, including per-target observed
  outcomes, timeouts, cardinality, baseline comparison and aggregate ratchet.
- Docker-gated integration targets and their execution evidence.
- Repository-input classification requiring an integration run bound to the
  current head, event and checkout/base provenance. The legacy verifier remains
  fail-closed on pushes to main; its pending-run retry is push-only.

Library unit tests and clippy do not prove those integration properties. A public
PR green result therefore does not establish full integration, deployment or
launch readiness. Integration Target Ratchet, manual CI Readiness and runner
smoke jobs cannot run for pull requests; their persistent runners are guarded
for push-to-main or explicit manual dispatch only. Main ratchet path filters
remain aligned with the repository-input manifest.

Private-only CI steps retired by snapshot exclusion are archive-only checks.
Hosted workflows are prepared now and held for the owner's Wednesday activation.
No paid larger runner is selected. Rust results remain unverified until actual
nonzero-step hosted or authorized CT102 runs complete.
