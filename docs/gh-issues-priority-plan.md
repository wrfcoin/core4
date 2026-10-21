# WRFCoin GitHub Issues Priority Plan

> 28 open issues in `wrfcoin/core4` as of 2026-02-21
> Organized into 12 work sessions, prioritized by impact and dependency chains

---

## Session 1 — P0: Unblock CI (Billing + Broken YAML) - Done PR312

These two must be resolved first. Every other CI issue is blocked by #311, and #312 is broken independently of billing.

### \#311 — Fix GitHub Actions billing to unblock all CI workflows - Done

**Priority:** P0 / urgent
**What:** All GitHub Actions fail with a billing/spending limit error. Update payment method or increase spending limit in GitHub Settings > Billing \& plans. No code changes required — purely administrative.
**Key files:** None (GitHub account settings)
**Blocks:** #312, #313, #314, #315, #316, #317

### \#312 — Fix broken workflow YAML files - Done - worktree: wild-tickling-haven

**Priority:** P1 / bug
**What:** Three workflows have structural YAML errors independent of billing:

* `release-signing.yml` — uses a reusable workflow (`slsa-framework/slsa-github-generator`) as a step instead of a job-level `uses:`
* `automated-rollback.yml` — triggers on push-to-main despite only defining `workflow\\\_call`/`workflow\\\_dispatch` *(historical: this workflow was later deleted entirely, 2026-03-16, #760/#763 — it no longer exists)*
* `component-discoverability.yml` — likely needs quoting on numeric comparisons or `dorny/paths-filter@v3` compat fix

**Key files:**

* `.github/workflows/release-signing.yml`
* `.github/workflows/automated-rollback.yml` *(deleted 2026-03-16, #760/#763)*
* `.github/workflows/component-discoverability.yml`

---

## Session 2 — P1: Core CI Pipelines (Readiness + Supply Chain) - Done PR 312

Once billing is resolved, validate the two gatekeeper pipelines that block every PR.

### \#313 — Validate and fix CI Readiness pipeline - Done - worktree: wild-tickling-haven

**Priority:** P1
**What:** `ci-readiness.yml` is the primary PR quality gate (6 jobs: preflight, lint-format, typecheck, security, tests, readiness-gate). Known issues: 230+ clippy warnings in `blockchain-core`, 82 npm audit vulnerabilities will fail `--audit-level=moderate`, PR title scope list may be incomplete.
**Key files:**

* `.github/workflows/ci-readiness.yml`
* `blockchain-core/` (clippy warnings)
* `package-lock.json` (npm audit)

### \#314 — Validate and fix Supply Chain Security pipeline - Done - worktree: wild-tickling-haven

**Priority:** P1
**What:** `supply-chain-security.yml` runs 6 checks (OSV Scanner, cargo-audit, cargo-vet, dep-budget, typosquatting, security-gate). Issues: workflow references Rust 1.89.0 but project uses 1.88.0, `supply-chain/` directory may not exist, `scripts/check-dep-budget.sh` needs verification.
**Key files:**

* `.github/workflows/supply-chain-security.yml`
* `scripts/check-dep-budget.sh`
* `blockchain-core/Cargo.toml`

---

## Session 3 — P2: Remaining CI Workflows - Done PR 321

Batch the remaining CI issues together — they're all workflow configuration/enablement.

### \#315 — Validate Rust Runtime Parity Gate and CLI Integration tests - done - worktree: wild-tickling-haven

**Priority:** P2
**What:** Two Rust CI workflows need test file verification: `rust-runtime-parity-gate.yml` (runtime parity between `wrfcoin-native-chain` and `wrfcoin-blockchain-core`) and `wrfcoin-cli-integration.yml` (mocked + live tests). Verify referenced test files exist.
**Key files:**

* `.github/workflows/rust-runtime-parity-gate.yml`
* `.github/workflows/wrfcoin-cli-integration.yml`
* `blockchain-core/tests/runtime\\\_protocol\\\_namespace\\\_parity\\\_gate.rs`
* `tools/cli/WrfcoinCLI/tests/api\\\_client\\\_integration.rs`
* `tools/cli/WrfcoinCLI/tests/command\\\_group\\\_integration.rs`
* `tools/cli/WrfcoinCLI/scripts/live\\\_contract\\\_smoke.sh`

### \#316 — Enable and configure skipped CI checks - done - worktree: wild-tickling-haven

**Priority:** P2
**What:** 8 workflows are skipped/inactive. For each, decide: enable on PRs, scheduled, manual-only, or remove. Workflows: `dependabot-auto-merge.yml`, `docker-security-scan.yml`, `sbom-generate.yml`, `adversarial-tests.yml`, `disaster-recovery-test.yml`, `gap-report-automation.yml`, `deploy-gate.yml`, `branch-protection-setup.yml`.
**Key files:**

* `.github/workflows/dependabot-auto-merge.yml`
* `.github/workflows/docker-security-scan.yml`
* `.github/workflows/sbom-generate.yml`
* `.github/workflows/adversarial-tests.yml`
* `.github/workflows/disaster-recovery-test.yml`
* `.github/workflows/gap-report-automation.yml`
* `.github/workflows/deploy-gate.yml`
* `.github/workflows/branch-protection-setup.yml`

### \#317 — Fix Chromatic Visual Testing and Deploy to Production workflows - done - worktree: wild-tickling-haven

**Priority:** P2
**What:** Two workflows: (1) `chromatic-optimized.yml` for visual regression of 450+ components — needs `CHROMATIC\\\_PROJECT\\\_TOKEN` and working Storybook build. (2) `deploy-production.yml` for multi-region AWS EKS — needs AWS credentials, ECR, EKS clusters, Terraform backend.
**Key files:**

* `.github/workflows/chromatic-optimized.yml`
* `.github/workflows/deploy-production.yml`
* `frontend/.storybook/` (Storybook config)

---

## Session 4 — P0: Bridge Safety Hardening - Done PR 322

Standalone session — this is critical production security work in Rust.

### \#205 — Bridge safety hardening: auto-halt, exposure caps, challenge windows - done - worktree: dazzling-floating-rainbow

**Priority:** P0 / high-priority
**What:** Complete bridge safety controls. Initial risk controls (auto-halt + caps) already exist in `src/cross\\\_chain/bridge.rs`. Remaining: explicit challenge windows for inbound/outbound claims, per-chain and global exposure caps with governance thresholds, anomaly auto-halt wired to operator/governance unhalt, stuck-state and partial-finality handling, emergency runbooks and alerting hooks.
**Key files:**

* `src/cross\\\_chain/bridge.rs` (existing auto-halt + caps)
* `blockchain-core/layer2/bridge/mod.rs`
* `blockchain-core/layer2/bridge/facade.rs`
* `blockchain-core/layer2/bridge/engines/deposit\\\_engine.rs`
* `blockchain-core/layer2/bridge/engines/withdrawal\\\_engine.rs`
* `blockchain-core/layer2/bridge/monitoring/health\\\_checker.rs`
* `backend/shared/bridge-verification-orchestrator.ts`
* `backend/shared/bridge-finality-monitor.ts`

---

## Session 5 — P1: Frontend Trust \& Safety UX (Cross-Layer) - Done PR 323

These three P1 issues share a common theme: exposing backend readiness/safety state in the frontend. Work them together.

### \#235 — Cross-layer trust clarity: runtime capability registry - done - worktree:starry-napping-emerson

**Priority:** P1
**What:** Replace static feature flags with a runtime capability registry. Backend endpoint exposes per-capability readiness (unavailable/simulated/local-manager/production-verified/degraded). Frontend consumes registry to drive route visibility, trust badges, and degraded-mode messaging.
**Key files:**

* `frontend/src/config/features.ts` (current static flags)
* `blockchain-core/api/handlers/` (where capability endpoint would live)
* `frontend/src/components/` (badge components)

### \#257 — Network ops UX: guardrails and confirmations for risky actions - done - worktree:starry-napping-emerson



**Priority:** P1
**What:** Operations dashboards expose sensitive actions without safety guardrails. Classify risky operations across validator/network dashboards, require explicit confirmations, add preflight checks (readiness, role/permission, policy state), record audit events.
**Key files:**

* `frontend/src/components/NetworkInsights/`
* `frontend/src/components/Network/`
* `blockchain-core/api/handlers/security.rs`
* `backend/server/routes/`

### \#273 — Upgrade/fork readiness UX: protocol transition messaging - done - worktree:starry-napping-emerson



**Priority:** P1
**What:** During protocol upgrades/forks, users lack clear messaging. Add backend transition state endpoint (planned/in-progress/compatibility-window/finalization/rollback), frontend UX banners across wallet/governance/bridge/operator views, gate risky actions during transitions.
**Key files:**

* `blockchain-core/api/handlers/blockchain.rs`
* `frontend/src/pages/` (affected views)
* `frontend/src/components/WeatherDashboard/`

---

## Session 6 — P2: Frontend UX Polish (Explorer + Forecast + Trust)

Five P2 frontend-heavy issues that share similar patterns (badges, timelines, reliability signals).

### \#271 — Explorer IA consolidation: unify /explorer, /dag-explorer, /network

**Priority:** P2
**What:** Three explorer entry points lack a canonical flow. Create one primary landing route with redirects/breadcrumbs from legacy paths, capability badges, and updated route ownership docs.
**Key files:**

* `frontend/src/components/explorer/BlockchainExplorer.tsx`
* `frontend/src/components/DAGExplorer/DAGExplorer.tsx`
* `frontend/src/components/BlockExplorerDAG/DAGBlockExplorer.tsx`
* `frontend/src/components/NetworkInsights/`
* `frontend/src/ROUTE\\\_OWNERSHIP.md`

### \#258 — Forecast UX: runtime reliability signaling

**Priority:** P2
**What:** Forecast/analysis surfaces have varying maturity. Add reliability states (verified/provisional/degraded/unavailable) as badges/messages in Forecast Viewer, Studio, Pipeline, and Data Explorer. Tie to backend capability/health and data freshness.
**Key files:**

* `frontend/src/components/WeatherIntelligence/`
* `frontend/src/components/DataExplorer/DataExplorer.tsx`
* `frontend/src/components/UnifiedAnalytics/UnifiedAnalyticsDashboard.tsx`

### \#259 — Trust provenance: unified verification timeline

**Priority:** P2
**What:** Build a normalized verification timeline model (issuance, attestation, validation, revocation, dispute, finality). Reusable timeline component with links to evidence (hashes, signatures, block refs). Degraded mode for missing evidence.
**Key files:**

* `frontend/src/components/` (new timeline component)
* `blockchain-core/consensus/proof\\\_of\\\_publishing.rs`
* `backend/shared/bridge-verification-orchestrator.ts`

### \#272 — Contract trust UX: preflight simulation and risk explanation

**Priority:** P2
**What:** Standardized preflight simulation for high-risk actions (governance mutation, bridge finalize/challenge, DeFi high-risk tx). Show expected state transitions, policy/permission checks, risk factors, confidence hints before execution.
**Key files:**

* `frontend/src/pages/SmartContractsPage.tsx`
* `frontend/src/components/Staking/LiquidStakingDashboard.tsx`
* `blockchain-core/api/handlers/governance.rs`

### \#274 — API trust UX: idempotency, replay-protection, webhook delivery

**Priority:** P2
**What:** Expose reliability metadata in API responses (fresh/replay/conflict, nonce validation, webhook delivery state). Frontend surfaces for this metadata. Observability hooks for delivery retries and replay rejection.
**Key files:**

* `blockchain-core/api/` (response middleware)
* `src/api/auth.rs` (nonce/replay protection)
* `backend/server/routes/`

---

## Session 7 — P2: Backend Data Systems

### \#213 — Data lineage + provenance API from telemetry to forecast artifacts

**Priority:** P2 / high-priority
**What:** End-to-end data lineage from raw sensor packet to forecast artifact. Immutable lineage IDs, signed metadata at each transformation stage, query APIs for provenance traversal and export, integrity checks for missing/forged segments. Missing lineage should block reward settlement.
**Key files:**

* `blockchain-core/weather/` (weather validation)
* `backend/server/enhanced\\\_weather\\\_rewards.ts`
* `backend/api/` (new lineage endpoints)
* `blockchain-core/consensus/proof\\\_of\\\_publishing.rs`

### \#275 — Gap-report automation: CI-generated interaction release notes

**Priority:** P2
**What:** Gap status documents drift from code. Build a deterministic report-generation pipeline, CI validation to prevent drift, release-note output covering resolved gaps, residual gaps, and changed risk/readiness status.
**Key files:**

* `reports/frontend-interaction-mapping-summary.md`
* `reports/wrfcoin-rust-blockchain-feature-gap-analysis-2026-02-15.md`
* `.github/workflows/gap-report-automation.yml`

---

## Session 8 — CLI Foundation: Agent-Friendly API + Output Formatting

These two create the foundation that all other CLI issues build on.

### \#282 — Agent-friendly API design: --json flag, structured errors, machine-readable output

**Priority:** Enhancement
**What:** Global `--json` / `--output {json,table,csv}` arg, structured exit codes (0=success through 5=blockchain error), stdin/pipe support for batch operations, `--yes` for non-interactive mode, standardized REST API response envelopes, OpenAPI spec generation.
**Key files:**

* `tools/cli/WrfcoinCLI/src/main.rs`
* `tools/cli/WrfcoinCLI/src/cli/mod.rs`
* `tools/cli/WrfcoinCLI/src/cli/commands.rs`
* `tools/cli/WrfcoinCLI/src/output.rs`
* `tools/cli/WrfcoinCLI/Cargo.toml`

### \#286 — CLI: one-command contributor workflow (`wrfcoin contribute start`)

**Priority:** Enhancement
**What:** Single command to run station checks, submission loop, and reward tracking. Includes preflight checks, optional loop mode, reward and quality summary output. JSON mode for machine readability.
**Key files:**

* `tools/cli/WrfcoinCLI/src/commands/weather.rs`
* `tools/cli/WrfcoinCLI/src/commands/` (new `contribute.rs`)
* `tools/cli/WrfcoinCLI/src/api.rs`

---

## Session 9 — CLI/TUI: Monitoring \& Dashboard

These four share the CLI/TUI monitoring domain. Build the ratatui dashboard first, then layer the specialized views.

### \#281 — ratatui TUI dashboard for node operators and weather station owners

**Priority:** Enhancement
**What:** TUI with 4 tabs: Dashboard (node sync, peers, hashrate), Weather Station (readings, quality scores, earnings), Network (peers, bridge status), Wallet (balance, transactions, staking). Uses `ratatui` + `crossterm`, accessible via `wrfcoin tui`.
**Key files:**

* `tools/cli/WrfcoinCLI/Cargo.toml` (add ratatui + crossterm deps)
* `tools/cli/WrfcoinCLI/src/commands/monitor.rs` (existing data fetching)
* `tools/cli/WrfcoinCLI/src/` (new `tui/` module)

### \#285 — CLI/TUI: `wrfcoin live` split-pane dashboard

**Priority:** Enhancement
**What:** Always-on terminal dashboard with split panes for wallet, node, weather rewards, and bridge health. Keyboard shortcuts, graceful degraded mode, 80x24 fallback, clean exit.
**Key files:**

* `tools/cli/WrfcoinCLI/src/` (new `live` command, builds on #281 TUI)
* `tools/cli/WrfcoinCLI/src/commands/monitor.rs`

### \#287 — CLI/TUI: operator incident mode with actionable status

**Priority:** Enhancement
**What:** `wrfcoin monitor incident` or `wrfcoin operator` with red/yellow/green status and remediation steps. Health checks across API, node, bridge, data pipelines. Severity scoring, runbook hints, non-zero exit for critical incidents.
**Key files:**

* `tools/cli/WrfcoinCLI/src/commands/monitor.rs`
* `tools/cli/WrfcoinCLI/src/commands/` (new `operator.rs` or extend monitor)

### \#288 — CLI: offline-first weather queue with signed replay-safe sync

**Priority:** Enhancement
**What:** Local durable queue for offline weather submissions. Signature/nonce metadata for replay protection. `wrfcoin weather sync` command, conflict handling, duplicate suppression, inspectable queue state.
**Key files:**

* `tools/cli/WrfcoinCLI/src/commands/weather.rs`
* `tools/cli/WrfcoinCLI/src/config.rs` (local storage paths)
* `tools/cli/WrfcoinCLI/src/api.rs`

---

## Session 10 — CLI: Transaction Builder + Cross-Chain Trading

### \#289 — CLI: Interactive transaction builder with dry-run and risk checks

**Priority:** Enhancement
**What:** `wrfcoin wallet send --interactive` with fee simulation, route/bridge preview, warnings (high fee, unknown recipient, low balance), confirmation gate, dry-run printing exact payload and estimated outcomes.
**Key files:**

* `tools/cli/WrfcoinCLI/src/commands/wallet.rs`
* `tools/cli/WrfcoinCLI/src/commands/bridge.rs`
* `tools/cli/WrfcoinCLI/src/api.rs`

### \#283 — WRF token cross-chain liquidity: WRF-to-BTC/USDC trading path

**Priority:** Enhancement
**What:** Functional trading path: WRF/USDC AMM pool, cross-chain bridge integration (WRF bridgeable to Ethereum/Polygon), WRF/USD price oracle, CLI `wrfcoin wallet swap` command, "How to cash out" docs.
**Key files:**

* `smart-contracts/` (AMM pool contracts)
* `backend/server/enhanced\\\_crosschain\\\_server.ts`
* `backend/server/oracle/enhanced-cross-chain-bridge-manager.ts`
* `tools/cli/WrfcoinCLI/src/commands/wallet.rs`
* `backend/server/enhanced\\\_weather\\\_rewards.ts`

---

## Session 11 — MCP Server for AI Agent Integration

Standalone session — new package creation.

### \#279 — Build WRFCoin MCP server for AI agent integration

**Priority:** Enhancement
**What:** MCP server exposing WRFCoin operations as tools for AI agents. Tools: weather data (submit, batch, station status, quality score), forecasts (publish, ensemble, verify), wallet (balance, send, history), network (node/bridge status), carbon credits. TypeScript using `@modelcontextprotocol/sdk`, calls existing REST APIs, publishable as `npx wrfcoin-mcp`.
**Key files:**

* `backend/server/mcp/mcpServer.ts` (existing MCP implementation to evaluate/extend)
* `backend/server/mcp/modelRegistry.js`
* `backend/server/mcp/contextProvider.js`
* `.mcp.json` (project MCP config)

---

## Session 12 — Meta: Adoption Strategy

This is a coordination/planning issue, not a coding session. Address after the above are underway.

### \#284 — Vertical takeoff strategy: developer experience and viral adoption

**Priority:** Enhancement
**What:** Meta-issue describing the adoption strategy. Priority order: working CLI (`cargo install wrfcoin`), TUI dashboard (#281), MCP server (#279), WRF-to-BTC path (#283), README overhaul. Defines "60-second wow" experience and metrics to track (stars/day, cargo installs, weather readings/day).
**Key files:** `README.md`, plus references issues #278, #279, #280, #281, #282, #283

---

## Execution Order Summary

|Order|Session|Issues|Est. Scope|Dependency|
|-|-|-|-|-|
|1|CI Unblock|#311, #312|Small (admin + YAML fixes)|None — do first|
|2|Core CI Pipelines|#313, #314|Medium|Blocked by #311|
|3|Remaining CI|#315, #316, #317|Medium|Blocked by #311|
|4|Bridge Safety|#205|Large (Rust security)|Independent|
|5|Frontend Trust P1|#235, #257, #273|Large (full-stack)|Independent|
|6|Frontend UX P2|#271, #258, #259, #272, #274|Large (full-stack)|Benefits from #235|
|7|Backend Data|#213, #275|Medium-Large|Independent|
|8|CLI Foundation|#282, #286|Medium (Rust CLI)|Independent|
|9|CLI/TUI Monitoring|#281, #285, #287, #288|Large (ratatui)|Benefits from #282|
|10|CLI Trading|#289, #283|Medium-Large|Benefits from #282|
|11|MCP Server|#279|Medium (new package)|Independent|
|12|Adoption Strategy|#284|Planning only|After #281, #279, #283|

Sessions 1-3 are sequential (dependency chain). Sessions 4-11 can be parallelized across worktrees. Session 12 is a planning pass once the building blocks exist.

