# Native Chain Launch Plan (Option B)

## Overview
This plan implements a fair and free launch on the native chain with earned-stake locking, DAO governance, decentralized oracle attestations, and commit-reveal to prevent manipulation.

## Launch Path Summary
- Canonical chain is the native runtime in this repo.
- Rewards are minted only after valid reveal plus oracle quorum.
- Early rewards fill stake requirement before becoming spendable.
- Governance controls emission, oracle set, quorum, and validation windows.

## Launch Readiness Delta (February 8, 2026)
Completed hardening items:
- Reward quorum now counts unique oracle IDs and rejects duplicate same-oracle attestations.
- Reward mint path now depends on accepted state transitions (commit/reveal/attestation), not raw tx payload counts.
- Oracle signatures are now mandatory across launch paths (API admission, mempool admission, PoP scoring, and state attestation transitions).
- Governance and direct state transitions now enforce safe oracle invariants:
  - non-empty oracle set
  - oracle IDs must be ed25519 public key hex strings
  - quorum greater than zero
  - quorum less than or equal to set size
  - commit/reveal governance windows must remain non-zero and ordered (`commit_window <= reveal_window`).
- Mempool admission now blocks malformed launch payloads, replay payload keys, and oversized oracle attestation payloads.
- Launch API now enforces JSON payload size limits, normalized JSON error responses, and strict launch-field validation.
- Launch observability endpoints added:
  - `GET /transactions/oracle/status`
  - `GET /transactions/stake/metrics`

Launch go/no-go gates:
1. `cargo test -p wrfcoin-native-chain` passes.
2. `cargo build -p wrfcoin-native-chain --bin launch_api` passes.
3. `tests/integration/native-chain-launch-api.test.js` passes against live `launch_api`.
4. `npm run dev:mvp` smoke startup succeeds.

## Ramifications
Security
- Strong protocol enforcement of fairness.
- Oracle quorum and slashing are consensus-critical.
- Commit-reveal reduces front-running but adds protocol complexity.

Economics
- Emissions are protocol-controlled and DAO-adjustable.
- Earned-stake locking reduces early liquidity but improves Sybil resistance.
- Accuracy-weighted rewards require careful tuning.

User Experience
- Commit-reveal requires two-step submission.
- Rewards are locked until stake is filled.
- Clear UX is required for stake progress and reveal windows.

Infrastructure Costs
- Requires native node fleet and decentralized oracle infrastructure.
- Monitoring and attestation coordination become core ops.

Regulatory Factors
- Work-based, permissionless issuance improves fairness narrative.
- Legal risk still depends on distribution, governance centralization, and jurisdiction.

## Concrete Checklist
- Make the native chain buildable and testable.
- Unify transaction model across consensus, mempool, and state execution.
- Add commit-reveal for weather data and forecast outputs.
- Add decentralized oracle attestations with quorum and slashing.
- Implement earned-stake locking so early rewards fill stake requirement.
- Add on-chain DAO governance and gating.
- Enforce data validation before rewards are minted.
- Add anti-Sybil and anti-gaming checks.
- Integrate economics configuration with governance.
- Update APIs and UX for commit-reveal and locked balance.
- Add tests for commit-reveal, oracle quorum, stake lock, and governance updates.
- Document launch rules and governance process.

## Staged Implementation Roadmap (Concrete PRs)

## Implementation Status (as of February 8, 2026)
- `PR 0` Buildable Native Chain Baseline: `Complete`
- `PR 1` Transaction Model Unification: `Complete`
- `PR 2` Commit-Reveal Plumbing: `Complete`
- `PR 3` Decentralized Oracle Attestations: `Complete`
- `PR 4` Earned-Stake Locking: `Complete`
- `PR 5` DAO Governance Controls: `Complete`
- `PR 6` Slashing and Anti-Gaming: `Complete`
- `PR 7` API and UX Updates: `Complete`
- `PR 8` Tests and Launch Docs: `Complete`

### Current Gaps
- Cross-module API cleanup remains for older non-launch endpoints (legacy metrics and integration surfaces).
- Frontend polish for non-MVP launch operations remains optional (protocol coverage is in API + tests).

### New PR7/PR8 Deliverables
- Unified API handler runtime extraction onto `ApiContext` for blockchain, transaction, address, NFT, and dashboard metrics handlers.
- Added contribution status APIs:
- `GET /transactions/contributions/status`
- `GET /transactions/commit/{commit_id}/status`
- Hardened reveal request DTOs with optional client timestamps for deterministic commit-hash checks in API tests.
- Added frontend fair-launch status page at `/fair-launch` with:
- Stake maturity progress (required/locked/spendable/earned)
- Commit/reveal/attestation queue summary and live table
- Commit-id lookup card for per-commit status
- Added integration test `tests/integration/native-chain-launch-api.test.js` covering:
- Stake-status schema read
- Commit/reveal/attestation submission flow and contribution-status lookup
- Added oracle config APIs for end-to-end quorum control in tests and ops:
- `GET /transactions/oracle/config`
- `PUT /transactions/oracle/config`
- Added native-chain launch API runtime for CI and local protocol validation:
- `cargo run -p wrfcoin-native-chain --bin launch_api`
- Added CI workflow for non-skipped launch API e2e:
- `.github/workflows/native-chain-launch-api.yml`
- Added launch operations runbook:
- `docs/runbooks/NATIVE_CHAIN_LAUNCH_RUNBOOK.md`

PR 0: Buildable Native Chain Baseline
- Goal: ensure the native chain can compile and run tests.
- Changes
- Add a proper Cargo package for the native chain runtime or move the root `src/` into a new crate.
- Align workspace members in `Cargo.toml`.
- Add minimal build and test CI scripts if missing.
- Primary files
- `Cargo.toml`
- New runtime crate `node/Cargo.toml` and `node/src/*` if created.

PR 1: Transaction Model Unification
- Goal: use one transaction model across consensus, mempool, and state.
- Changes
- Add missing `TransactionType`, `TransactionInput`, `TransactionOutput` to `src/blockchain/transaction.rs` or refactor to use a shared module.
- Add contribution transaction types.
- Update consensus modules to reference the unified model.
- Primary files
- `src/blockchain/transaction.rs`
- `src/consensus/proof_of_publishing.rs`
- `src/mempool/pool.rs`
- `src/api/handlers/transactions.rs`

PR 2: Commit-Reveal Plumbing
- Goal: commit-reveal at protocol level for data and forecasts.
- Changes
- Add `src/contributions/commit_reveal.rs` with commit validation, reveal windows, and replay protection.
- Extend `State` with commit and reveal registries.
- Add transaction handling for `ContributionCommit` and `ContributionReveal`.
- Primary files
- `src/contributions/commit_reveal.rs`
- `src/blockchain/state.rs`
- `src/blockchain/transaction.rs`
- `src/consensus/proof_of_publishing.rs`

PR 3: Decentralized Oracle Attestations
- Goal: enforce oracle quorum for reward eligibility.
- Changes
- Add `src/oracle/attestation.rs` for oracle set, quorum, and signature verification.
- Add state for `oracle_set`, `oracle_quorum`, `attestations`.
- Only accept reveal-based rewards when quorum is met.
- Primary files
- `src/oracle/attestation.rs`
- `src/blockchain/state.rs`
- `src/consensus/proof_of_publishing.rs`
- `src/common/constants.rs`

PR 4: Earned-Stake Locking
- Goal: rewards fill stake requirement first, then become spendable.
- Changes
- Extend `Account` and `StateTransition` for `stake_required`, `stake_locked`, `spendable_balance`.
- Route reward issuance into stake lock until requirement met.
- Primary files
- `src/blockchain/state.rs`
- `src/blockchain/token_emission.rs`

PR 5: DAO Governance Controls
- Goal: on-chain governance gating for parameters and oracles.
- Changes
- Extend governance proposals to update `oracle_set`, `oracle_quorum`, `stake_required`, `emission_rate`, commit/reveal windows.
- Integrate governance decisions into state transitions.
- Primary files
- `src/governance/mod.rs`
- `src/governance/implementation.rs`
- `src/blockchain/state.rs`
- `src/common/constants.rs`

PR 6: Slashing and Anti-Gaming
- Goal: penalize dishonest data and oracle misbehavior.
- Changes
- Add slashing rules for invalid reveal, low-quality data, and fraudulent attestation.
- Decrease `stake_locked` and mark offenders for cooldown.
- Primary files
- `src/security/slashing.rs` or extend existing security modules.
- `src/blockchain/state.rs`

PR 7: API and UX Updates
- Goal: clear UX for commit-reveal and locked balance.
- Changes
- Add API endpoints for commit, reveal, attestation, stake status, locked vs spendable balance.
- Add client UX elements to show commit status and stake progress.
- Primary files
- `src/api/handlers/transactions.rs`
- `frontend/*` or relevant client code

PR 8: Tests and Launch Docs
- Goal: enforce invariants and document launch rules.
- Changes
- Add tests for commit-reveal, quorum, stake lock, and governance updates.
- Document fair launch rules and governance process.
- Primary files
- `tests/*` or `src/**/tests`
- `docs/native-chain-launch-plan.md`
