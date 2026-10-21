# GitHub Actions Workflow Justification

## Owner relevance and nightly ruling — 2026-10-07 (Refs #2498)

This section supersedes the historical trigger rows below for the 16 workflows listed here.
Live `rules/branches/main` requires only `schedule-lint`, `detect-secrets baseline audit (merge tree)`,
`verify-terminal-skip-guards`, and `repository-input integration admission`. All four workflows,
`integration-ratchet.yml`, and `.github/repository-input-admission.json` are unchanged.
Admission reads only integration-ratchet's two jobs; none of these 16 supplies required evidence.

PR selectors retain every former path and add missing inputs. Local dependency closures include
normal, build, dev, target-specific and optional dependencies without assuming feature exclusions.
The workspace-wide deploy check and serde-json feature scan select every workspace member.
Shared-protocol test closures also select the repository input formats its tree scan reads:
`.env`, `.json`, `.jsonl`, `.toml`, `.yml`, `.yaml`, `.sh`, `.conf`. Script/test inputs are
conservatively included as `scripts/**` and `tests/**`, including helper imports, baselines and
fixtures; `.github/actions/**`, all Cargo manifests, lockfile, toolchain and `.cargo/**` select
full scope. This intentionally prefers extra runs over an under-covered dependency or fixture.

Daily schedules use the default branch (main), the existing jobs and evidence floors. Phase 3C's
localhost API remains opt-in dispatch-only; adversarial compilation remains schedule/dispatch-only,
with its corpus guard on selected PRs. Terminal manifest `default` partitions already cover schedule;
no conditional classifications or floors change. Unique non-PR run-id concurrency groups prevent
nightly/main runs from cancelling or evicting retained evidence. No deployment/settings changes.

| Workflow | Old PR scope | New PR scope / actual read inputs | Nightly UTC | Evidence absent on excluded PRs |
|---|---|---|---|---|
| `tdd-phase3c.yml` | `.github/workflows/tdd-phase3c.yml`, `scripts/run-phase3b-security-api.sh`, `scripts/run-phase3c-security-api.sh`, `shared-protocol/**`, `tests/security/**`, `Cargo.lock`, `Cargo.toml` | `.github/workflows/tdd-phase3c.yml`, `scripts/run-phase3b-security-api.sh`, `scripts/run-phase3c-security-api.sh`, `shared-protocol/**`, `tests/security/**`, `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `build-provenance/**`, `consensus-engines/**`, `da/**`, `dataset-manifest/**`, `evm/**`, `execution-engine/**`, `forecast-score/**`, `geo-grid/**`, `meteo-types/**`, `native-chain/**`, `p2p/**`, `primitives/**`, `privacy/**`, `reputation-engine/**`, `tools/cli/WrfcoinCLI/**`, `zk-circuits/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 02:00 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `build-provenance-gate.yml` | `build-provenance/**`, `scripts/attest_build.sh`, `scripts/cargo_test_nonvacuous.py`, `scripts/test_cargo_test_nonvacuous.py`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.github/workflows/build-provenance-gate.yml` | `build-provenance/**`, `scripts/attest_build.sh`, `scripts/cargo_test_nonvacuous.py`, `scripts/test_cargo_test_nonvacuous.py`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.github/workflows/build-provenance-gate.yml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `scripts/**`, `tests/**` | 02:11 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `ci-deploy-features.yml` | `native-chain/**`, `shared-protocol/**`, `p2p/**`, `consensus-engines/**`, `primitives/**`, `Cargo.toml`, `Cargo.lock`, `docs/security/RPC_SURFACE.md`, `scripts/check_rpc_surface.py`, `scripts/launch_api_routes.py`, `scripts/check_launch_api_spec.py`, `scripts/test_check_launch_api_spec.py`, `docs/api/launch-api.openapi.yaml`, `rust-toolchain.toml`, `scripts/check_toolchain_pin.py`, `**/Dockerfile*`, `.github/workflows/**` | `native-chain/**`, `shared-protocol/**`, `p2p/**`, `consensus-engines/**`, `primitives/**`, `Cargo.toml`, `Cargo.lock`, `docs/security/RPC_SURFACE.md`, `scripts/check_rpc_surface.py`, `scripts/launch_api_routes.py`, `scripts/check_launch_api_spec.py`, `scripts/test_check_launch_api_spec.py`, `docs/api/launch-api.openapi.yaml`, `rust-toolchain.toml`, `scripts/check_toolchain_pin.py`, `**/Dockerfile*`, `.github/workflows/**`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `.github/workflows/ci-deploy-features.yml`, `build-provenance/**`, `da/**`, `dataset-manifest/**`, `evm/**`, `execution-engine/**`, `forecast-score/**`, `geo-grid/**`, `keeper-proof/**`, `meteo-types/**`, `oracle-packet/**`, `privacy/**`, `reputation-engine/**`, `sdks/generated/rust/**`, `security-demo/**`, `sensor-attestation/**`, `tests/integration/**`, `tools/cli/WrfcoinCLI/**`, `tools/forecast-proto/**`, `zk-circuits/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 02:22 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `unsafe-policy.yml` | `primitives/**`, `shared-protocol/**`, `consensus-engines/**`, `scripts/check_unsafe_justified.py`, `scripts/unsafe_baseline.json`, `docs/security/UNSAFE_POLICY.md`, `.github/workflows/unsafe-policy.yml` | `primitives/**`, `shared-protocol/**`, `consensus-engines/**`, `scripts/check_unsafe_justified.py`, `scripts/unsafe_baseline.json`, `docs/security/UNSAFE_POLICY.md`, `.github/workflows/unsafe-policy.yml`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `scripts/**` | 02:33 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `native-chain-package-gate.yml` | `native-chain/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `scripts/package_test_baseline_gate.py`, `scripts/test_package_test_baseline_gate.py`, `scripts/package_test_baselines.json`, `.github/workflows/native-chain-package-gate.yml` | `native-chain/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `scripts/package_test_baseline_gate.py`, `scripts/test_package_test_baseline_gate.py`, `scripts/package_test_baselines.json`, `.github/workflows/native-chain-package-gate.yml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `build-provenance/**`, `consensus-engines/**`, `da/**`, `dataset-manifest/**`, `evm/**`, `execution-engine/**`, `forecast-score/**`, `geo-grid/**`, `meteo-types/**`, `p2p/**`, `primitives/**`, `privacy/**`, `reputation-engine/**`, `shared-protocol/**`, `tools/cli/WrfcoinCLI/**`, `zk-circuits/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 02:44 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `determinism-lint.yml` | `consensus-engines/src/**`, `primitives/src/**`, `shared-protocol/src/**`, `native-chain/src/**`, `scripts/check_determinism.py`, `scripts/determinism_baseline.json`, `scripts/determinism_repair_probe.sh`, `.github/workflows/determinism-lint.yml`, `**/Cargo.toml`, `Cargo.lock`, `scripts/check_serde_json_float_features.py` | `consensus-engines/src/**`, `primitives/src/**`, `shared-protocol/src/**`, `native-chain/src/**`, `scripts/check_determinism.py`, `scripts/determinism_baseline.json`, `scripts/determinism_repair_probe.sh`, `.github/workflows/determinism-lint.yml`, `**/Cargo.toml`, `Cargo.lock`, `scripts/check_serde_json_float_features.py`, `Cargo.toml`, `rust-toolchain.toml`, `.cargo/**`, `.github/actions/**`, `build-provenance/**`, `consensus-engines/**`, `da/**`, `dataset-manifest/**`, `evm/**`, `execution-engine/**`, `forecast-score/**`, `geo-grid/**`, `keeper-proof/**`, `meteo-types/**`, `native-chain/**`, `oracle-packet/**`, `p2p/**`, `primitives/**`, `privacy/**`, `reputation-engine/**`, `sdks/generated/rust/**`, `security-demo/**`, `sensor-attestation/**`, `shared-protocol/**`, `tests/integration/**`, `tools/cli/WrfcoinCLI/**`, `tools/forecast-proto/**`, `zk-circuits/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 02:55 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `bincode-boundary-guard.yml` | `**/*.rs`, `scripts/check_bincode_boundary_attrs.py`, `scripts/test_check_bincode_boundary_attrs.py`, `.github/workflows/bincode-boundary-guard.yml` | `**/*.rs`, `scripts/check_bincode_boundary_attrs.py`, `scripts/test_check_bincode_boundary_attrs.py`, `.github/workflows/bincode-boundary-guard.yml`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `scripts/**` | 03:06 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `cli-test-gate.yml` | `**/*.rs`, `**/Cargo.toml`, `Cargo.lock`, `.github/workflows/cli-test-gate.yml` | `**/*.rs`, `**/Cargo.toml`, `Cargo.lock`, `.github/workflows/cli-test-gate.yml`, `Cargo.toml`, `rust-toolchain.toml`, `.cargo/**`, `.github/actions/**`, `build-provenance/**`, `dataset-manifest/**`, `forecast-score/**`, `geo-grid/**`, `meteo-types/**`, `primitives/**`, `privacy/**`, `reputation-engine/**`, `shared-protocol/**`, `tools/cli/WrfcoinCLI/**`, `zk-circuits/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 03:17 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `wrfcoin-cli-integration.yml` | `.github/workflows/wrfcoin-cli-integration.yml`, `native-chain/src/bin/launch_api/**`, `tools/cli/WrfcoinCLI/**` | `.github/workflows/wrfcoin-cli-integration.yml`, `native-chain/src/bin/launch_api/**`, `tools/cli/WrfcoinCLI/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `build-provenance/**`, `consensus-engines/**`, `da/**`, `dataset-manifest/**`, `evm/**`, `execution-engine/**`, `forecast-score/**`, `geo-grid/**`, `meteo-types/**`, `native-chain/**`, `p2p/**`, `primitives/**`, `privacy/**`, `reputation-engine/**`, `shared-protocol/**`, `zk-circuits/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 03:28 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `launch-api-bin-tests.yml` | `native-chain/**`, `shared-protocol/**`, `p2p/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.github/workflows/launch-api-bin-tests.yml` | `native-chain/**`, `shared-protocol/**`, `p2p/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.github/workflows/launch-api-bin-tests.yml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `build-provenance/**`, `consensus-engines/**`, `da/**`, `dataset-manifest/**`, `evm/**`, `execution-engine/**`, `forecast-score/**`, `geo-grid/**`, `meteo-types/**`, `primitives/**`, `privacy/**`, `reputation-engine/**`, `tools/cli/WrfcoinCLI/**`, `zk-circuits/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 03:39 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `crate-execution-gates.yml` | `consensus-engines/**`, `shared-protocol/**`, `privacy/**`, `zk-circuits/**`, `execution-engine/**`, `evm/**`, `meteo-types/**`, `geo-grid/**`, `forecast-score/**`, `dataset-manifest/**`, `keeper-proof/**`, `da/**`, `oracle-packet/**`, `sensor-attestation/**`, `reputation-engine/**`, `primitives/**`, `tests/integration/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `scripts/cargo_test_nonvacuous.py`, `scripts/test_cargo_test_nonvacuous.py`, `scripts/check_crate_execution_floor.py`, `scripts/test_check_crate_execution_floor.py`, `.github/workflows/crate-execution-gates.yml` | `consensus-engines/**`, `shared-protocol/**`, `privacy/**`, `zk-circuits/**`, `execution-engine/**`, `evm/**`, `meteo-types/**`, `geo-grid/**`, `forecast-score/**`, `dataset-manifest/**`, `keeper-proof/**`, `da/**`, `oracle-packet/**`, `sensor-attestation/**`, `reputation-engine/**`, `primitives/**`, `tests/integration/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `scripts/cargo_test_nonvacuous.py`, `scripts/test_cargo_test_nonvacuous.py`, `scripts/check_crate_execution_floor.py`, `scripts/test_check_crate_execution_floor.py`, `.github/workflows/crate-execution-gates.yml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `build-provenance/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 03:50 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `named-test-targets.yml` | `native-chain/tests/oracle_governance_rotation_2712.rs`, `native-chain/tests/oracle_governance_declaration_2712.rs`, `native-chain/tests/chain_identity_split_2713.rs`, `native-chain/tests/chain_identity_fail_closed_2713.rs`, `native-chain/tests/restored_producer_set_is_unbound_2290.rs`, `shared-protocol/src/crypto/domain_signing.rs`, `native-chain/src/bin/launch_api/main.rs`, `scripts/check_named_test_targets.py`, `scripts/named_test_targets.json`, `.github/workflows/named-test-targets.yml`, `native-chain/tests/cut_composition_lifecycle_2498.rs`, `shared-protocol/src/consensus/proof_of_publishing.rs`, `native-chain/src/mining/collaborative_pools.rs`, `scripts/cargo_test_nonvacuous.py`, `scripts/test_cargo_test_nonvacuous.py` | `native-chain/tests/oracle_governance_rotation_2712.rs`, `native-chain/tests/oracle_governance_declaration_2712.rs`, `native-chain/tests/chain_identity_split_2713.rs`, `native-chain/tests/chain_identity_fail_closed_2713.rs`, `native-chain/tests/restored_producer_set_is_unbound_2290.rs`, `shared-protocol/src/crypto/domain_signing.rs`, `native-chain/src/bin/launch_api/main.rs`, `scripts/check_named_test_targets.py`, `scripts/named_test_targets.json`, `.github/workflows/named-test-targets.yml`, `native-chain/tests/cut_composition_lifecycle_2498.rs`, `shared-protocol/src/consensus/proof_of_publishing.rs`, `native-chain/src/mining/collaborative_pools.rs`, `scripts/cargo_test_nonvacuous.py`, `scripts/test_cargo_test_nonvacuous.py`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `build-provenance/**`, `consensus-engines/**`, `da/**`, `dataset-manifest/**`, `evm/**`, `execution-engine/**`, `forecast-score/**`, `geo-grid/**`, `meteo-types/**`, `native-chain/**`, `p2p/**`, `primitives/**`, `privacy/**`, `reputation-engine/**`, `shared-protocol/**`, `tools/cli/WrfcoinCLI/**`, `zk-circuits/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 04:01 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `input-panic-lint.yml` | `**/*.rs`, `scripts/check_no_panic_on_input.py`, `scripts/panic_input_baseline.json`, `scripts/panic_input_baseline.migrations`, `.github/workflows/input-panic-lint.yml` | `**/*.rs`, `scripts/check_no_panic_on_input.py`, `scripts/panic_input_baseline.json`, `scripts/panic_input_baseline.migrations`, `.github/workflows/input-panic-lint.yml`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `scripts/**` | 04:12 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |
| `workflow-path-coverage.yml` | every PR | `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `.github/workflows/workflow-path-coverage.yml`, `.github/**`, `scripts/**` | 04:23 | Unrelated source/prose PRs no longer get path-coverage/inventory execution. Required structural/admission checks still run. |
| `retired-component-sdks.yml` | every PR | `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `.github/workflows/retired-component-sdks.yml`, `**/*package.json`, `**/*.md`, `sdks/extensions/**` | 04:34 | PRs without package manifests, Markdown, guard/SDK or global inputs no longer get retired-SDK claim scans. |
| `adversarial-tests.yml` | `native-chain/tests/**`, `consensus-engines/tests/**`, `scripts/adversarial_suite.py`, `.github/workflows/adversarial-tests.yml` | `native-chain/tests/**`, `consensus-engines/tests/**`, `scripts/adversarial_suite.py`, `.github/workflows/adversarial-tests.yml`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/**`, `**/Cargo.toml`, `.github/actions/**`, `build-provenance/**`, `consensus-engines/**`, `da/**`, `dataset-manifest/**`, `evm/**`, `execution-engine/**`, `forecast-score/**`, `geo-grid/**`, `meteo-types/**`, `native-chain/**`, `p2p/**`, `primitives/**`, `privacy/**`, `reputation-engine/**`, `shared-protocol/**`, `tests/integration/**`, `tools/cli/WrfcoinCLI/**`, `zk-circuits/**`, `scripts/**`, `tests/**`, `**/*.env`, `**/.env`, `**/.env.*`, `**/*.json`, `**/*.jsonl`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.sh`, `**/*.conf` | 04:45 | Existing unrelated prose exclusions remain; widened filters remove no former PR evidence. |

The path-coverage manifest pins global inputs and each closure, including CLI dev-dependency inputs
previously excluded by native-chain package/bin gates. The Phase 3C entry is now reviewed; the
unreviewed ceiling falls from two to one. `test_relevance_nightly.py` recalculates the local graph
and rejects missing dependency sources/fixtures, duplicate/non-daily/out-of-window crons, missing
dispatch, invalid workflow keys, lost nightly verdicts, or filtered required contexts.


> **Status note (2026-07-23, re-reconciled against current `main`; issue #2321):** the #2334
> correction pass (anchored at `1974b0719`) fixed the staleness the #2322 audit found, but the
> same night's merges re-staled three entries — #2088 gave `tdd-phase3c` PR/push triggers, #2337
> made `supply-chain-security` scanner failures fatal, #2338 made `docker-security-scan`
> blocking — and `kani.yml` landed with no entry. This pass corrects those against the YAML on
> current `main`. The tree today has **27 workflow files** — 13 detailed entries plus the
> [Uncatalogued workflows](#uncatalogued-workflows) table (14 rows; `release-gate-fixtures.yml`
> added by #2331).
>
> *(Prior note, 2026-07-22, kept for history: the #2322 audit found this catalogue extensively
> stale — five wrong trigger claims, a ghost entry for a workflow deleted in February, a
> 3-for-4 stale cross-cutting section, and unlisted workflows. The recurring cause of drift was
> cost-driven trigger removals (e.g. `08a0a793`, `27103f64`, #2233) that updated the YAML but
> never this file: **when a trigger changes, change the entry in the same PR.**)*

## Overview

On 2026-02-14, workflows were consolidated from **35 to 14** to reduce CI noise, eliminate
duplication, and remove workflows referencing non-existent infrastructure. On 2026-02-21,
three overlapping CI workflows (`turbo-ci.yml`, `quality-gates.yml`, `pr-checks.yml`) were
further consolidated into the unified `ci-readiness.yml` (issue #241).

**Before**: 35 workflows generating ~116 checks per push (67 failing, 46 skipped, 3 cancelled)
**After**: 13 workflows with clear, non-overlapping responsibilities
*(+1 on 2026-03-09: `tdd-phase3c.yml` added for Phase 3C TDD gate)*
*(−4 on 2026-03-16: removed automated-rollback, chromatic-optimized, component-discoverability,
gap-report-automation — broken after frontend/backend extraction, issue #760)*

---

## Detailed entries (13 of the 26 current workflows; the other 13 are in the Uncatalogued table)

### CI & Quality (2 workflows)

#### 1. `ci-readiness.yml` - Unified CI Pipeline
- **Triggers**: paths-filtered `pull_request` (Rust surface: `**/*.rs`, Cargo files, toolchain
  pin, the workflow itself), weekly schedule (Mon 04:00 UTC), `workflow_dispatch`. **No `push`
  trigger.** On a qualifying PR, four jobs run unconditionally: `preflight`, `preflight-rust`,
  `rust-lint`, `rust-tests` (#2233/#2265 narrowed the PR path deliberately for Actions cost;
  the JS/contract/report jobs are guarded off `pull_request` events).
- **Purpose**: Unified CI pipeline replacing the former `turbo-ci.yml`, `quality-gates.yml`, and
  `pr-checks.yml`. Runs lint, format, typecheck, PR title validation, lockfile integrity, security
  scans, Rust build verification, smart contract compilation, coverage checks, and dependency review.
- **Justification**: Single source of truth for CI checks, eliminating duplication across three
  overlapping workflows. Created as part of issue #241 CI readiness hygiene.
- **Replaces**: `turbo-ci.yml`, `quality-gates.yml`, `pr-checks.yml` (all deleted Feb 2026).

#### 2. `tdd-phase3c.yml` - Phase 3C TDD Gate
- **Triggers** *(rewritten by #2088, merged 2026-07-23)*: path-filtered `pull_request` and
  `push [main]` (this workflow, the phase3b/3c scripts, `shared-protocol/**`,
  `tests/security/**`, Cargo manifests — `tdd-phase3c.yml:36-58`), weekly schedule
  (Mon 03:00 UTC), and `workflow_dispatch` with a **required** `run_api_tests` boolean input
  (`:59-64`). **PR-reachable** on path-matching PRs. *(A prior revision said weekly +
  input-less dispatch with no PR reach — true pre-#2088, wrong now.)*
- **Purpose**: Incremental TDD gate for Phase 3C quality-slashing and reputation-reward tests.
  Runs pure Rust integration tests (TDD-12.2–12.5: `slashing_integration`,
  `reward_integration` + two shared-protocol tests) on every trigger; the API leg (TDD-12.1)
  builds and runs an ephemeral localhost `launch_api` and is gated on
  `workflow_dispatch` + `run_api_tests=true` + the protected `testnet-api` environment.
- **Jobs**: `tdd-phase3c-rust`, `tdd-phase3c-api`, `phase3c-summary` *(the former
  `tdd-phase3c-optional` job no longer exists; TDD-12.6–12.11 are dispatch-only API tests
  listed in the in-file header)*.
- **Justification**: Separates concern from `ci-readiness.yml`; locks Phase 3C consensus
  contracts as non-regression gates. Pre-#2088 the Rust leg carried two real test failures
  (#2083, still open); #2088 restored the gate — merged inside the hosted-runner starvation, so
  the repaired gate is undemonstrated in CI until its first genuine run
  (`docs/security/GATE_STATUS.md` row 21).
- **Local equivalent**: `bash scripts/run-phase3c-security-api.sh [--rust-only]`; probe
  `scripts/phase3c_probe.sh`
- **Added**: 2026-03-09; repaired 2026-07-23 (#2088)

### Security (6 workflows)

#### 4. `supply-chain-security.yml` - Supply Chain Security
- **Triggers**: weekly schedule (Mon 04:00 UTC) and `workflow_dispatch` **only** — the push/PR
  triggers for dependency files were removed by `08a0a793` for Actions cost (#2316 records that a
  PR changing the dependency surface no longer invokes this gate).
- **Purpose**: Comprehensive supply chain security: OSV scanner, cargo-audit, cargo-vet, dependency
  budget enforcement, typosquatting detection, automated issue creation for vulnerabilities.
- **Jobs**: osv-scanner, cargo-audit, cargo-vet, dependency-budget, typosquatting-check,
  security-gate, create-vulnerability-issues
- **Justification**: Most comprehensive security workflow. Critical for a blockchain/financial
  platform. Weekly schedule (Mon 04:00) and manual dispatch only, so low noise.
- **Known issues** *(corrected 2026-07-22)*: this workflow uses no `github-script` action at all,
  and `scripts/check-dep-budget.sh` exists on `main` — both prior claims here were wrong (#2322).
- **Update 2026-07-23**: #2337 (merged 00:11Z) made scanner failures **fatal** and cleared the
  permanent-red 31/30 dependency-budget baseline (#2316). Merged during the hosted-runner
  starvation: no genuine run of the fixed config yet — next Mon 04:00 or a dispatch demonstrates
  (`docs/security/GATE_STATUS.md` row 20).

#### 5. `docker-security-scan.yml` - Docker Vulnerability Scanning
- **Triggers**: weekly schedule (Sun 02:00 UTC) and `workflow_dispatch` **only** — the
  path-scoped push/PR triggers were removed by `08a0a793` for Actions cost; `pull_request` was
  **deliberately not restored** by #2338 (in-file rationale, `docker-security-scan.yml:5-16`).
- **Purpose**: Discovers Dockerfiles, builds images, scans with Trivy for HIGH/CRITICAL
  vulnerabilities, validates docker-compose files, uploads SARIF to GitHub Security tab.
- **Jobs**: discover-dockerfiles, trivy-scan (matrix), config-scan, compose-validation, summary
- **Justification**: Docker-specific scanning that other security workflows don't cover; weekly
  + dispatch keeps CI noise minimal.
- **Enforcement** *(rewritten by #2338, merged 03:35Z 2026-07-23)*: image scan and config scan
  are now **blocking** (`exit-code: '1'` + `severity: CRITICAL,HIGH`, `:120`/`:177`) —
  supersedes the #2312-era state where the only blocking step was guarded by an unreceivable
  `pull_request` event. The image **build's** `continue-on-error` is retained *deliberately* so
  per-image classification still runs (`:88-95` comment); Trivy is pinned to an immutable SHA,
  no longer `@master`. Merged during the hosted-runner starvation: zero genuine runs of the
  blocking config yet (`docs/security/GATE_STATUS.md` row 9).

#### 6. `sbom-generate.yml` - Software Bill of Materials
- **Triggers**: **release-candidate tag push (`v*.*.*`)** — the pre-publication gate — plus
  `release: published` (attach-only), weekly schedule (Wed 05:00 UTC), and `workflow_dispatch`
  (with a `scan_vulnerabilities` boolean). The `release: published` event fires *after* a release
  is public, so it can only turn red afterwards; the authoritative gate is the tag, which runs the
  full validate + critical-scan before a release is cut (review of #2347). Still **no branch `push`
  trigger** (dependency changes are caught weekly; release safety on the tag gate).
- **Purpose**: Generates CycloneDX SBOMs for npm and Rust dependencies, validates them fail-closed,
  scans for critical vulnerabilities with Grype, uploads SARIF to GitHub Security.
- **Jobs**: generate-sbom (validate + scan **before** publish/attach), verify-sbom (redundant
  re-validation of the published artifact).
- **Enforcement (#2317, #2347)**: all gates run inside `generate-sbom` **before** upload/attach —
  `scripts/validate_sbom.py` rejects a missing/empty/malformed inventory (incl. the
  `components`-as-string bypass); `cyclonedx validate --fail-on-errors` runs before publish; the
  committed `scripts/sbom_scan_probe.sh` negative control proves a critical scanner result is not
  swallowed; Grype `--fail-on critical` runs under `pipefail` with no `continue-on-error`. A bad
  SBOM is never uploaded or attached.
- **Justification**: SBOM generation is increasingly required for regulatory compliance (especially
  for financial/blockchain platforms). Low noise (release + weekly + manual).
- **Known issues** *(corrected 2026-07-22)*: the CycloneDX format-name concern is unverified;
  the `upload-artifact@v3` claim is stale — no v3 usage remains anywhere in
  `.github/workflows/` (#2322).

#### 6a. `determinism-lint.yml` - Consensus determinism lint
- **Triggers**: PR and push-to-main on `consensus-engines/src/**`, `primitives/src/**`,
  `shared-protocol/src/**`, `native-chain/src/**`, plus the detector script and its baseline.
- **Purpose**: Blocks NEWLY-introduced nondeterministic constructs (floats, wall-clock reads,
  unseeded RNG, unordered-map iteration, unsorted `read_dir`, consensus-path env reads) from the
  consensus-critical crates. Baseline mode: fails only on findings not already frozen.
- **Justification**: Cheap and deterministic (Python, seconds), so it can gate every PR that touches
  the scanned directories. The trigger paths are asserted to cover `SCANNED_ROOTS` **per trigger**
  by `--check-workflow-paths`, because the two lists are in different languages and drifted apart
  once already — omitting `shared-protocol/src/blockchain`, the live consensus state code.
- **Acceptance probe**: `scripts/determinism_lint_probe.sh`. Docs: `docs/security/DETERMINISM_LINT.md`.

#### 6b. `unsafe-policy.yml` - `// SAFETY:` baseline gate
- **Triggers**: PR on `primitives/**`, `shared-protocol/**`, `consensus-engines/**`, the gate
  script, the baseline, or the policy doc; weekly schedule; `workflow_dispatch`.
- **Purpose**: Requires every `unsafe` in non-test code to carry a `// SAFETY:` justification,
  baselined per site so pre-existing debt does not block PRs while new violations do.
- **Justification**: Cheap (Python, seconds) and deterministic, so it *can* run on every PR —
  unlike Miri. Split out of `miri.yml` under #2106 so that a PR run of the Miri workflow cannot
  exist and therefore cannot be miscited as Miri evidence.
- **Acceptance probe**: `scripts/unsafe_miri_probe.sh`.

#### 6c. `miri.yml` - Undefined-behaviour detection
- **Triggers**: weekly schedule + `workflow_dispatch`. **Deliberately no `pull_request`.**
- **Purpose**: `cargo +nightly miri test` over `wrf-primitives` and
  `wrfcoin-shared-protocol`'s `crypto::` modules — the remaining UB-prone surfaces named by
  #2106. #2454 retired the `consensus-engines` FFI bridge and its `ffi::` Miri matrix leg; this
  catalogue must not present scheduled runs as current consensus-engine coverage.
- **Justification**: Miri is an interpreter; a whole-crate `shared-protocol` run measured 2h23m on
  a GitHub runner before failing. Running it per-PR is unaffordable against the current Actions
  budget, so it is scheduled. Its trigger set and its name now agree, which is the point: the
  earlier combined workflow reported PR runs as `success` with every Miri leg `skipped`.
- **Cite only `schedule`/`workflow_dispatch` runs as Miri evidence.** See
  `docs/security/UNSAFE_POLICY.md`.

### Deployment (1 workflow)

#### 7. `deploy-production.yml` — **DELETED; entry removed 2026-07-22**
This catalogue listed `deploy-production.yml` as a kept workflow for five months after the file
was deleted (`d256a6628`, 2026-02-23, the #373 extraction) — the ghost-entry failure mode #2309
documents for `automated-rollback.yml`. Row added to the Deleted table below; no replacement
production-deploy workflow exists, and the real deploy path is local builds via SSH to the N5
(`docs/security/EMERGENCY_RUNBOOK.md`, `docs/security/GENESIS_CUT_PLAN.md` §5).

#### 8. `deploy-gate.yml` - manual tool; **gates no deployment** *(reclassified 2026-08-31, #2890)*
- **Triggers**: workflow_dispatch only. Declared in-file as
  `# NO-AUTOMATIC-TRIGGER: manual`, enforced by `scripts/check_workflow_inventory.py`.
- **What it actually is**: a decorative workflow. **Zero runs in its entire lifetime**, nothing
  invokes it, and the real deploy path — local builds pushed by SSH to the N5 host — involves no
  GitHub-hosted workflow at all (PR #2320,
  `docs/plans/2026-07-22-deploy-gate-exposure.md`).
- **Jobs**: smoke-tests, performance-check, security-audit, release-endpoints (#2139: builds the
  release binary as shipped and fails if any `dev-endpoints` route literal is in it; the decision
  does not accept `skipped` for it), deploy-decision — none of which has ever executed, so it is
  also unknown whether they *can*.
- **Do not cite this file as deployment or security coverage.** The prior entry here called it a
  "useful safety check before production deployments"; that sentence is what #2890 is about. Two
  structural reasons it would not gate anything even if dispatched, both readable in the file:
  1. `security-audit` cannot fail. `npm audit` runs under `|| true`, the findings branch sets
     `result=warn` and returns 0, and the secret scan emits `::warning::` only. It has no failing
     exit path.
  2. `deploy-decision` accepts `skipped` for both `performance-check` and `security-audit`, so a
     skipped security job approves the deploy.

  A third, environmental: `performance-check` runs `npm start` and polls
  `http://localhost:4001/health`, but core4's root `start` script is `next start` and the API was
  extracted to `wrfcoin/backend` — `start:all` now literally echoes that. Deleting the file is the
  arguable end state and is left as an owner decision, because it would also strand
  `scripts/deploy_gate_smoke.sh` and the `deploy-gate` assertions in `scripts/test_release_gates.py`
  (line 449), which `release-gate-fixtures.yml` runs today.
- **Known issues** *(corrected 2026-07-22 — the prior list here named only trivia while the gate
  was broken)*: the gate approved failed smoke suites and non-starting servers from its creation
  commit until #2304 fixed it to fail closed (#2301); it has **zero lifetime runs**, nothing
  invokes it, and the real N5 deploy path does not route through it (PR #2320). The fixture
  harness `scripts/test_release_gates.py` is now wired into `release-gate-fixtures.yml` on a
  path-filtered PR+push trigger (#2331), so a regression in either gate's verification logic
  fails a PR instead of merging with the harness unrun. Legacy notes (`bc` availability,
  secret-regex false positives) unverified.

### Release & Automation (4 workflows)

#### 12. `release-signing.yml` - Secure Release Signing
- **Triggers**: Release created, workflow_dispatch
- **Purpose**: Builds release binaries for 5 platforms, signs the binaries and combined checksum
  manifest with GPG, attaches them to GitHub releases, and verifies the detached signatures.
- **Jobs**: build-binaries (matrix: 5 platforms; since #2139 each build also runs
  `scripts/check_release_endpoints.py --binary` on the artifact so a release cannot ship the dev
  endpoints), sign-binaries, attach-to-release, verify-signatures
- **Justification**: Cryptographic signing is essential for a blockchain platform. Users need to
  verify binary authenticity. Zero CI noise (only on releases).
- **Assurance boundary** *(updated 2026-07-23 for #2263)*: the workflow authenticates binaries and
  `CHECKSUMS.txt` under the existing GPG release key. It does **not** produce SLSA/Sigstore
  provenance, use a transparency log, or independently rebuild-and-compare artifacts, and therefore
  makes no reproducibility claim. The former unsigned `PROVENANCE.json`, unused OIDC permission, and
  unverified `"reproducible": true` assertion were removed rather than presented as assurance.
- **Known issues** *(updated 2026-07-23)*: the signature-verification step accepted fully
  unsigned artifact sets until #2304 replaced the discovered-`*.asc` loop with an
  expected-artifact-set derivation (#2300); the fixture harness is now invoked by
  `release-gate-fixtures.yml` (#2331), and `security/SIGNING_KEY.asc` — the documented in-repo key
  fallback — is absent on `main` (#2300). Legacy notes (macOS/Windows `sha256sum`,
  `SOURCE_DATE_EPOCH`) remain unverified.
- **Requires**: `GPG_PRIVATE_KEY`, `GPG_PASSPHRASE`, `GPG_KEY_ID` secrets.

#### 13. `dependabot-auto-merge.yml` - Dependabot Auto-Merge
- **Triggers**: pull_request_target (Dependabot PRs only)
- **Purpose**: Automatically approves and merges Dependabot PRs based on update type, dependency
  category, compatibility score (>90), and security status. Blocks critical dependency updates.
- **Jobs**: auto-merge-dependabot
- **Justification**: Reduces manual toil for routine dependency updates. Only triggers on Dependabot
  PRs - zero noise for human-authored PRs.
- **Known issues**: Compatibility score check may fail on empty string; auto-merge requires repo
  setting to be enabled.

#### 14. `branch-protection-setup.yml` — **DELETED 2026-09-01 (#2960 story 4 of the merge-admission epic)**
- Deleted, not parked. It was a `workflow_dispatch`-only tool (#2967 made it manual) whose one job
  `PUT`s **legacy** branch protection with `strict: true`, `enforce_admins: true`, two required
  reviews and **nine required contexts that no workflow produces** (`pr-validation`,
  `smoke-tests`, `security-scan`, `coverage-check`, `dependency-review`, `Security Scanning`,
  `Test Blockchain Core`, `Test Backend Services`, `Test Frontend`). Anyone with `workflow_dispatch`
  rights could therefore lock every merge on `main` behind checks that can never report, with
  admins enforced. Meanwhile the real admission authority is the `main-protection` ruleset
  (19875840), which this tool never read. A tool that asserts one protection state while a
  different one is live is the #2960 shape — a green tick about a tree (here: a policy) that
  nobody evaluated — so it goes, and the ruleset is documented directly in
  `docs/ci/MERGE_ADMISSION.md` instead. Its last run on `main` (33448711648, 2026-08-31T22:59Z,
  `push`) completed `failure` with **zero jobs**. `.github/scripts/validate-cicd-setup.sh`, which
  asserted this file *and* the long-deleted `deploy-production.yml` exist and told the reader to
  dispatch this tool, is deleted with it (no callers; it has exited 1 since 2026-02-23).

#### 15. `sprint-stats.yml` - Sprint Velocity Dashboard
- **Triggers**: Weekly Friday schedule and workflow_dispatch
- **Purpose**: Runs `scripts/sprint-stats.sh` to aggregate merged PR velocity by repo, open PR
  triage counts, and a generated sprint history CSV/report artifact. Optionally comments on the
  active sprint issue when `ACTIVE_SPRINT_ISSUE` or workflow input is configured.
- **Justification**: Replaces manual sprint close-out counting with one low-frequency GitHub API
  pass. Uses `WRFCOIN_STATS_TOKEN` when configured for cross-repo private reads; otherwise falls
  back to the repository token for current-repo/manual runs.
- **Local equivalent**: `bash scripts/sprint-stats.sh --sprint 520 --since 2026-05-20T07:00:00Z`

---

## Deleted Workflows (31) with Rationale

| Workflow | Reason for Deletion |
|----------|-------------------|
| `test-target-compile.yml` | Deleted 2026-09-01 (#2888 story 7, per-push load). **Not a coverage cut: its one step moved, verbatim, into `integration-ratchet.yml`** ("Compile every workspace test target (--no-run, --features hsm)", executable-count floor 122 included). The property it guards — every workspace test target, bin unit tests included, compiles under `hsm` alone (#2841's rot class, owner decision 2026-08-30) — is still asserted on the same runner label and the same `**/*.rs` trigger, now sequentially inside the ratchet job instead of as a separate concurrent job on a three-runner pool with a shared build cache. Traded: the result is reported under the ratchet job's context name, not its own; the compile shares the ratchet's 90-minute timeout |
| `branch-protection-setup.yml` | Deleted 2026-09-01 (#2960, merge-admission epic). Manual tool that would `PUT` legacy branch protection requiring nine contexts no workflow produces, with `enforce_admins: true` — a dispatch away from locking `main`. The `main-protection` ruleset (19875840) is the admission authority; see `docs/ci/MERGE_ADMISSION.md` |
| `automated-rollback.yml` | References unprovisioned ECS/ECR rollback infrastructure; removed Mar 2026 (#760) |
| `deploy-production.yml` | Deleted `d256a6628` (2026-02-23) in the #373 extraction; this catalogue wrongly kept its entry until 2026-07-22 (#2321/#2322). No replacement exists; real deploys are local SSH builds to the N5 |
| `chromatic-optimized.yml` | Frontend Chromatic visual testing — frontend extracted to wrfcoin/frontend; removed Mar 2026 (#760) |
| `component-discoverability.yml` | Frontend component scanning — frontend extracted to wrfcoin/frontend; removed Mar 2026 (#760) |
| `gap-report-automation.yml` | References `scripts/validate-gap-report.ts` which doesn't exist; removed Mar 2026 (#760) |
| `canary-deployment.yml` | Duplicates deploy-production.yml; AWS infrastructure doesn't exist |
| `chromatic.yml` | Superseded by chromatic-optimized.yml (itself now also deleted — frontend extracted) |
| `ci-cd-quality-gate.yml` | Duplicated CI checks (same triggers, same checks) |
| `codebase-analysis.yml` | Auto-commits to main without PR (bypasses all protections); missing scripts |
| `deploy-staging.yml` | References non-existent AWS ECS/ECR infrastructure; deploys to deprecated Goerli testnet |
| `deploy-with-component-health.yml` | No actual deployment (just echo statements); simulated health checks |
| `disaster-recovery-test.yml` | Missing `test-failover.sh` script; DR infrastructure doesn't exist |
| `e2e-testing.yml` | References non-existent test directories (`sdks/generated/`, `tests/e2e/`) |
| `enhanced-security-gate.yml` | Duplicates supply-chain-security.yml security scanning |
| `multi-environment-orchestration.yml` | Incomplete implementations ("Implement blue-green deployment..." comments); missing Dockerfiles |
| `native-chain-launch-api.yml` | `wrfcoin-native-chain` cargo package doesn't exist; integration test missing |
| `pr-checks.yml` | Superseded by `ci-readiness.yml` (Feb 2026, issue #241) |
| `preview-environments.yml` | Requires Vercel + Fly.io configs that don't exist; missing Dockerfiles |
| `production-deployment.yml` | Duplicates deploy-production.yml; references non-existent Helm charts and K8s manifests |
| `quality-gate-monitoring.yml` | Duplicated quality checks (same triggers) |
| `quality-gates.yml` | Superseded by `ci-readiness.yml` (Feb 2026, issue #241) |
| `regression-testing.yml` | Overlaps heavily with CI test execution |
| `sdk-validation.yml` | `sdks/generated/` directories don't exist; validates Java/C#/iOS/Android SDKs that aren't generated |
| `security.yml` | Duplicates supply-chain-security.yml (cargo-audit, npm audit, Snyk); hardcoded staging URL |
| `sonarqube-analysis.yml` | Requires SonarQube server (`SONAR_HOST_URL`) that isn't configured; path mismatch bug |
| `test-coverage.yml` | Duplicates CI test execution and coverage checks |
| `turbo-cache-setup.yml` | One-time setup utility; `npx turbo login` requires interactive input (fails in CI) |
| `turbo-ci.yml` | Superseded by `ci-readiness.yml` (Feb 2026, issue #241) |
| `weekly-maintenance.yml` | Missing `weekly_maintenance.sh` script; auto-commits to main without PR (dangerous) |

---

## Cross-Cutting Issues *(corrected 2026-07-22 against `1974b0719` — 3 of the 4 original claims
were stale or wrong, #2322)*

1. ~~Rust version inconsistency~~ **Done**: only `1.98.0` appears anywhere in
   `.github/workflows/`; the toolchain is pinned by `rust-toolchain.toml` and enforced by
   `scripts/check_toolchain_pin.py` (#2214).
2. ~~Deprecated `actions/upload-artifact@v3`~~ **Done**: zero v3 usages remain.
3. ~~`actions/github-script@v7` doesn't exist / latest is @v6~~ **Wrong in both directions**: v7
   and v8 both exist; the tree uses `@v8` ×4 and `@v7` ×1 (`fuzz-nightly.yml`); #1269 pinned
   v6→v8.
4. **Unpinned action versions** — narrowed 2026-07-23: the former example,
   `aquasecurity/trivy-action@master`, was SHA-pinned by #2338; the one remaining `@master`
   in the tree is `dtolnay/rust-toolchain@master` (`determinism-matrix.yml:91`).
5. **A job that can be skipped by its `if:` must have a static `name:`** (2026-09-01, #2981 /
   merge-admission epic story 5). GitHub never evaluates the `name:` template of a skipped job,
   so `name: foo (${{ cond && 'a' || 'b' }})` renders *literally* — `foo (${{ (((github.event_name
   == 'pull_request') && ...` — in the checks list, and a skipped-by-design job becomes
   indistinguishable from a broken workflow (this produced a false finding on #2981). Fixed in
   `secret-scanning.yml` (`secret-leak-scan`), `fuzz-nightly.yml` (`fuzz`, `notify-on-failure`)
   and `dependabot-auto-merge.yml`: the name is static and states the skip condition. Matrix jobs
   cannot drop the `${{ matrix.x }}` suffix, so they carry a stable prefix that says the skip is
   deliberate. The one template name deliberately kept is `detect-secrets` in
   `secret-scanning.yml`: it runs on both events (so it always expands) and its `(merge tree)`
   form is a **required context** in ruleset 19875840 — renaming it would break admission.

## Uncatalogued workflows

Eighteen workflows exist in this directory with no detailed entry above (the table below has grown past the count in the summary at the top of this file, which is stale — see #2911's row and `crate-execution-gates.yml`'s). Triggers below are read
from the YAML on `main` (last re-verified 2026-07-23); one line each, with the issue that owns
any known finding.

| Workflow | Triggers (verified) | What it is |
|---|---|---|
| `merge-admission-contract.yml` | push [main] (no filter) + PR (paths: the contract's five files) + dispatch | **ADVISORY live step since 2026-10-04 (owner ruling, #3198): the live comparison still runs and reports (step conclusion, `::warning`, job summary) but `continue-on-error: true` keeps it from failing the job; self-test and fixture tests remain blocking.** The control for `docs/ci/MERGE_ADMISSION.md`** (#2896 story 7): `scripts/check_merge_admission.py` reads the live `rules/branches/main`, fails on any difference from `.github/merge-admission.json` (required contexts, strict, rule set, PR params) or when the document stops naming a required context verbatim. Self-test + fixture tests run before the live step. Reads only; rulesets stay owner-only. Self-hosted, ~10 s |
| `secret-scanning.yml` | push `**` + PR [main] | detect-secrets baseline gate; the push+PR double-fire is **deliberate** and documented in a 27-line in-file comment (#2306) — do not "fix" it without reading that comment; non-hermeticity fixed by #2315 |
| `ci-deploy-features.yml` | PR + push [main], both paths-filtered | feature-matrix compile + `--no-run` test; also enforces the toolchain pin |
| `hsm-signer-ci.yml` | PR + push [main], both paths-filtered | `cargo test --features hsm` (softhsm2) |
| `input-panic-lint.yml` | PR + push [main], both paths-filtered | panic-on-input ban gate (#2248) |
| `fuzz-nightly.yml` | daily 07:00 + dispatch (`max_total_time` input) + PR (paths, build-smoke only) | nightly fuzz campaign; PR leg builds targets and replays the regression corpus (#2251) |
| `kani.yml` | PR (paths: `consensus-engines/**`, its doc, the workflow) + weekly Mon 06:00 + dispatch — **PR-reachable** | bounded model-checking of the economic arithmetic (`consensus-engines/src/economics/proofs.rs`, #2106/#2108): a cheap `proof-mirrors` test job plus per-harness Kani proofs under a 120 s process-group budget; see `docs/security/KANI_PROOFS.md` |
| `adversarial-tests.yml` | PR (paths-filtered) + daily 06:00 + dispatch | #2311 fixed: a cheap PR-reachable `corpus-guard` job fails closed if any adversarial target is missing (via `scripts/adversarial_suite.py --check-corpus` + `--self-test`); the heavy `adversarial-suite` job (schedule/dispatch only, per the 27103f64 cost decision) now runs the real adversarial `--test` targets (replay/partition/reorg/oracle/churn/bridge) and compares an ordered test-identity artifact across two runs, not a pass count |
| `rust-shared-native-parity.yml` | weekly Mon 05:00 + dispatch | invariant-detection + re-export gate as of #2310; behavioural oracles for four modules remain untracked-in-this-file (see #2330) |
| `determinism-matrix.yml` | PR + push [main], both paths-filtered + dispatch | cross-platform state-root comparison (#2104/#2243) |
| `loom.yml` | weekly Mon 04:00 + dispatch | loom interleaving models; **`ci-readiness`'s `--lib` gate does not run these — this workflow is their only CI home** (#2257) |
| `ci-self-hosted-smoke.yml` | push `ci/n5pro-**` + dispatch | smoke job for the self-hosted N5 runner (`n5pro-ct102`, #2324); runner capped 6 cores/12 GB per its in-file note |
| `sdks-extensions-javascript.yml` | PR + push [main], both paths-filtered + dispatch | JS SDK extension build/test plus cross-chain release-surface parity and mutation fixture (#3226) |
| `wrfcoin-cli-integration.yml` | PR (paths: the workflow, `native-chain/src/bin/launch_api/**`, `tools/cli/WrfcoinCLI/**`) + dispatch; the weekly Tue 04:00 schedule is PARKED (#2652) | CLI integration (mocked) + the launch_api boundary smoke; the live-contract smoke leg was removed by #2326 — see `docs/ci/CLI_LIVE_CONTRACT_COVERAGE.md` and #2328. **Both jobs moved to `[self-hosted, n5pro]` (#2921 decision 2, 2026-08-31; they were billing-VOID on `ubuntu-latest` since ~Aug 12)**, and the path filter never fires on `shared-protocol/**`, which the crate path-depends on — so this is not the CLI's test gate (#2741) |
| `cli-test-gate.yml` | PR + push [main] (paths: `**/*.rs`, `**/Cargo.toml`, `Cargo.lock`, the workflow) + dispatch | **the gate that RUNS `tools/cli/WrfcoinCLI`'s tests** (#2741), on `[self-hosted, n5pro]`. `cargo test --locked -p wrfcoin_cli` — every target, no `--lib`, no `--test <name>` — through `scripts/cargo_test_nonvacuous.py --min-executed 95` (103 executed on `09abcd20c`). Before it, the CLI's 82 lib+bin unit tests were executed by nothing on any runner; `test-target-compile.yml` compiles them `--no-run` and `wrfcoin-cli-integration.yml` is hosted |
| `crate-execution-gates.yml` | PR + push [main] (paths: the 16 covered crates, `Cargo.toml`/`Cargo.lock`, toolchain pin, the wrapper + its tests, the workflow) | **the gate that RUNS the 16 crates #2921 found compile-only or unexecuted**, on `[self-hosted, n5pro]`, one job, four sequential `cargo_test_nonvacuous.py` steps with measured floors (consensus-engines 420/5, shared-protocol 1900/45, privacy+zk+execution 200/13, leaf x11 180/24; measured at `044e7cc7f`, 2026-08-31). Owner decisions 1+3 (2026-08-31). Ships with its own concurrency group. Sequencing dependency: runner slots + concurrency groups on existing workflows should precede this volume (other lanes) |
| `native-chain-package-gate.yml` | PR + push [main] (PR: `native-chain/**`, `Cargo.toml`/`Cargo.lock`, toolchain pin, the gate script + its tests + its baseline manifest, the workflow; push widens to `**/*.rs`) + dispatch | **the gate that gives native-chain a package-wide signal again (#2911)**, on `[self-hosted, n5pro]`. `cargo test --locked --no-fail-fast -p wrfcoin-native-chain` through `scripts/package_test_baseline_gate.py`, which declares the four standing `chain_identity_split_2713` failures BY IDENTITY (#2713 is open; the RED is the proof) and classifies everything else: `NEW_FAILURE` / `HEALED` / `MISSING` = exit 1, gate-broken = exit 2. Before it, the package command was permanently exit 101 and — without `--no-fail-fast` — stopped at the 3rd of 45 integration targets, so a fresh regression produced an identical observable and 41 binaries never ran. Floors 1700/50, measured at `aa00107e8` 2026-08-31. Ships with its own concurrency group |
| `release-gate-fixtures.yml` | PR + push [main], both paths-filtered | runs `scripts/test_release_gates.py`, the release-signing (#2300) / deploy-gate (#2301) fixture harness, on any change to either gate script, the harness, or the two workflows it guards — so their verification logic cannot merge with the harness unrun; asserts gpg+pyyaml present so no case silently skips (#2331) |
| `declared-ceiling-enforcement.yml` | PR (paths: `constants.rs`, the #2701 test, this workflow) + push [main] (`**/*.rs`) + dispatch | **ADVISORY and expected RED.** Runs the #2701 test demonstrating #2700 — `MAX_BLOCK_SIZE`, `MAX_TRANSACTION_DATA_SIZE` and `MEMPOOL_CAPACITY` are consumed by zero lines of code. Deliberately **no `continue-on-error`**: that is wrapper-only success, which the evidence standard classifies VOID. Safe because this context is **not among main's required status checks**, not because none exist — as of 2026-08-31 ruleset `main-protection` (19875840) carries a `required_status_checks` rule with exactly three contexts (`schedule-lint`, `detect-secrets baseline audit (merge tree)`, `verify-terminal-skip-guards`), `strict_required_status_checks_policy: false`, and an OrganizationAdmin bypass. This job's context is absent from that list, so its red cannot block a merge. **If it is ever added to that list, park this workflow first** — re-verify with `gh api repos/wrfcoin/core4/rulesets/19875840`, not with `branches/main/protection`, which still returns 404 because the requirement lives in a ruleset rather than legacy branch protection. Step 1 is an anti-vacuity control that must pass, so step 2's red is attributable; a gate that breaks exits 2 and says the run is VOID rather than claiming a finding. **Retire it when it goes green**: remove the `#[ignore]` from the test (it then passes inside the unfiltered suites) and delete this file, because a permanently-green advisory is a stale exemption |

## Future Consolidation Recommendations

When billing is fixed and workflows can actually run:

1. **Combine `supply-chain-security.yml` + `docker-security-scan.yml`** into a single
   `security.yml` since both focus on vulnerability scanning.
2. **Target: 8-10 workflows** generating ~30-40 checks instead of 116.

> **Note (Feb 2026)**: The former `turbo-ci.yml`, `quality-gates.yml`, and `pr-checks.yml` have
> already been consolidated into `ci-readiness.yml` as part of issue #241.

---

## Registration inventory vs. files on disk *(#2890, snapshot `6ce83ec2f`, 2026-08-31)*

**The Actions API is not the gate inventory.** It lists *registrations*, and GitHub keeps a
registration after its file is deleted. At `6ce83ec2f`: **56 registrations (50 active, 6
`disabled_manually`) against 38 workflow files** — 18 registrations have no file here. An auditor
filtering on `state=active` sees 50 and reads reassuring names — `enhanced-security-gate`,
`automated-rollback` — that correspond to nothing. That wrong denominator is how the gaps in #2888
stayed invisible.

Regenerate this table at any time; it is a tool, not a transcription:

```bash
python3 scripts/check_workflow_inventory.py --reconcile
```

The registrations with no file on `main` fall into **three** classes, and the obvious action —
"delete the ones with no file" — is wrong for two of them:

| Class | Count | What it is | Action |
|---|---|---|---|
| `GITHUB_MANAGED` | 2 | `dynamic/dependabot/*` — not files; Dependabot's own workflows. `dependabot-updates` ran 402 times, most recently 2026-08-29. | **Never touch.** Still running and supposed to be. |
| `BRANCH_ONLY` | 6 | File exists on an unmerged branch, not on `main`: dispatchable on that ref. | None. Self-corrects on merge or branch deletion. |
| `GHOST` | 10 | No file on any branch. Historical runs only (Feb–Apr 2026), all recorded in the Deleted Workflows table above. | None available. **GitHub exposes no API to delete a workflow registration** — it ages out. Nothing to do but know the number. |

Two details worth keeping:

- `sdk-extensions-javascript.yml` (GHOST, 2 runs) and `sdks-extensions-javascript.yml` (live, 171
  runs) are the same workflow before and after a **file rename**. Renaming a workflow file creates a
  new registration and leaves the old one as a permanent ghost; renaming only the `name:` key does
  not. Prefer the latter.
- The 6 `disabled_manually` registrations are exactly the 6 core4 rows of
  `docs/ci/parked-workflows.txt` — accounted for, not drift.
