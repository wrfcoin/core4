# CI Billing / Spend Triage (2026-04-11)

**Issue:** [wrfcoin/core4#745](https://github.com/wrfcoin/core4/issues/745) — CI pipeline
non-functional due to billing/spend limits.

**Status of #745:** Closed 2026-04-05 ("Billing has been set up"). This doc is a **pre-emptive
spend-hotspot triage** for the next time the Actions budget comes under pressure. It is a
**read-only** inventory pass — no workflow YAML was modified. All recommendations need a human
with billing access to review and apply in a separate PR.

**Scope:** `wrfcoin/core4` only. Other repos (backend, frontend, infra, smart-contracts,
security-testing) should get parallel passes if budget pressure returns.

---

## 1. Context

Per the March 16 comment on #745, the team already responded to the initial billing event:

- Upgraded to Teams, added ~1,000 min of headroom for the cycle.
- Moved all 6 scheduled workflows to **weekly staggered** crons (Mon–Sun, different hours).
- Dropped per-PR triggers on everything except `dependabot-auto-merge.yml` and
  `sdks-extensions-javascript.yml` (the latter is path-scoped).
- Left `workflow_dispatch: {}` on every workflow for on-demand runs.

Pre-existing context: `.github/workflows/WORKFLOW_JUSTIFICATION.md` records that workflow count
was already pruned from 35 → 14 during Feb–Mar 2026. The easy cuts are already taken.

Today (2026-04-11) all `gh` API calls succeed — billing is not currently exhausted. This doc is a
**hardening pass**, not an emergency amputation.

---

## 2. Workflow Inventory

13 `.yml` files in `.github/workflows/` (plus `WORKFLOW_JUSTIFICATION.md`). Runtimes are
qualitative — no billing-portal access.

| File | Trigger(s) | Matrix | runs-on | Min/run | Rough monthly | Cut-risk |
|---|---|---|---|---|---|---|
| `adversarial-tests.yml` | schedule daily 06 UTC, dispatch | 1 | ubuntu | 10–15 (2× cargo test) | ~30 | MED |
| `branch-protection-setup.yml` | push-on-file-change, dispatch | 1 | ubuntu | <2 | ~1 | LOW |
| `ci-readiness.yml` | schedule Mon 04 UTC, dispatch | 6 jobs | ubuntu | 30–45 aggregate | ~4 | HIGH (main gate) |
| `dependabot-auto-merge.yml` | `pull_request_target` on every Dependabot PR | 1 | ubuntu | 1–3 | **~75/100 recent runs** | HIGH |
| `deploy-gate.yml` | dispatch only | 3 jobs + pg15/redis7 services | ubuntu | 10–15 | ~0 | LOW |
| `docker-security-scan.yml` | schedule Sun 02 UTC, dispatch | 1 + N×dockerfile (cap 20) | ubuntu | 10–30 × N | ~4 | MED |
| `release-signing.yml` | release created, dispatch | **5** | ubuntu-22.04 + **macos-14 ×2** + windows-2022 | 15–30/cell | ~1/release | HIGH |
| `rust-shared-native-parity.yml` | schedule Mon 05 UTC, dispatch | 1 | ubuntu | 8–15 | ~4 | MED |
| `sbom-generate.yml` | schedule Wed 05 UTC, dispatch | 2 jobs | ubuntu | 15–25 | ~4 | MED |
| `sdks-extensions-javascript.yml` | push/PR path-scoped, dispatch | 1 | ubuntu | 3–6 | ~10–40 | LOW |
| `supply-chain-security.yml` | schedule Mon 04 UTC, dispatch | 5 jobs | ubuntu | 30–60 aggregate | ~4 | MED |
| `tdd-phase3c.yml` | schedule Mon 03 UTC, dispatch | 3 jobs | ubuntu | 15–25 | ~4 | MED |
| `wrfcoin-cli-integration.yml` | schedule Tue 04 UTC, dispatch | 2 jobs | ubuntu | 15–25 | ~4 | MED |

**Macro observations:**

1. Eight workflows are **weekly cron only**. The easy schedule wins already happened Mar 16.
2. `dependabot-auto-merge.yml` is the **runaway-frequency** workflow (~75 of last 100 runs).
   Per-run cheap, fan-out high.
3. `release-signing.yml` is the **highest per-run cost** due to macos-14 (10× multiplier) × 2
   cells. Fires only on releases, so monthly cost is bounded but one release is expensive.
4. `ci-readiness.yml` is the **largest single-run** (6 jobs, tests job 30-min timeout). Weekly
   cron today so monthly impact is bounded to ~4 runs.
5. `sdks-extensions-javascript.yml` is already **path-scoped and concurrency-grouped** — the model
   other PR-triggered workflows should copy.

---

## 3. Spend Hotspots (ranked)

1. **`dependabot-auto-merge.yml`** — fires on every opened/synchronize/reopened Dependabot PR,
   ~75% of recent runs. `wait-on-check-action` polls at 30 s and bills wall-clock; concurrent
   synchronize events cause parallel wait-loops on the same PR.
2. **`ci-readiness.yml`** — 6 jobs, tests job compiles Hardhat + runs full
   `cargo test --workspace --lib --all-features` + adversarial cargo tests, 30–45 min aggregate
   per run. Weekly today; catastrophic if reverted to per-PR without splitting.
3. **`supply-chain-security.yml`** — 5 parallel jobs, source-compiles `cargo-audit` + `cargo-vet`
   on cache miss instead of using prebuilt binaries.
4. **`release-signing.yml`** — 5-cell matrix with macos-14 ×2 (10× multiplier) and windows-2022.
   `CARGO_BUILD_JOBS: "1"` enforced for reproducibility makes each build slower.
5. **`docker-security-scan.yml`** — `find . -name 'Dockerfile*' | head -20` auto-discovery, matrix
   grows linearly with Dockerfile count. `continue-on-error: true` on build masks failures.

Honorable mentions: `sbom-generate.yml` source-compiles `cargo-sbom` on miss.
`wrfcoin-cli-integration.yml` uses `Swatinem/rust-cache@v2` while other workflows use hand-rolled
`actions/cache@v5` — two cache shards = lower cross-workflow hit rate.

---

## 4. Recommended Immediate Cuts (low risk)

**Read-only recommendations. Apply in a separate PR.**

### 4.1 Add `concurrency` groups to cancel superseded runs

Workflows: `adversarial-tests.yml`, `ci-readiness.yml`, `rust-shared-native-parity.yml`,
`tdd-phase3c.yml`, `supply-chain-security.yml`, `docker-security-scan.yml`, `sbom-generate.yml`,
`wrfcoin-cli-integration.yml`.

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```

`sdks-extensions-javascript.yml` already has this — copy the pattern. Saves wasted minutes when
`workflow_dispatch` triggers while a cron run is mid-flight.

### 4.2 Use `cargo-binstall` for cargo-audit / cargo-vet / cargo-sbom

Workflows: `supply-chain-security.yml`, `sbom-generate.yml`. Current code runs
`cargo install cargo-audit --version 0.21.2 --locked` on cache miss (≥5 min source build per
tool). Replace with `cargo-binstall` or a pinned GitHub-release binary download. Worst-case
savings: ~15–20 min per affected run.

### 4.3 Concurrency group on `dependabot-auto-merge.yml`

```yaml
concurrency:
  group: dependabot-auto-merge-${{ github.event.pull_request.number }}
  cancel-in-progress: true
```

`wait-on-check-action` polls for up to the job timeout. A synchronize event during the wait
currently spawns a second parallel waiter; cancellation is safe because the workflow is
idempotent. Given this workflow is ~75% of all runs, even small per-run savings compound.

### 4.4 Cap / explicit-list `docker-security-scan.yml` matrix

Replace `find . -name 'Dockerfile*' | head -20` auto-discovery with an explicit file list or
`strategy.max-parallel: 4`. Bounds worst-case matrix growth and surfaces intent in PR review.

---

## 5. Recommended Medium-Risk Cuts (need team sign-off)

These reduce coverage or slow feedback. Do not apply without sprint sign-off.

- **Drop macos-14 aarch64 cell from `release-signing.yml`**: Saves ~150–300 min/release, but
  breaks signed builds for Apple Silicon users. Recommend **defer**.
- **Drop windows-2022 cell from `release-signing.yml`**: Saves ~30–60 min/release. Only if no
  Windows-native testnet operators — check with ops first.
- **Split `ci-readiness.yml` tests job into parallel Rust / Solidity lanes**: Halves wall-clock,
  adds cache-shard complexity. Worth doing independent of billing for feedback speed.
- **Consolidate `supply-chain-security.yml` + `docker-security-scan.yml` + `sbom-generate.yml`
  into one security workflow**: Already called out in `WORKFLOW_JUSTIFICATION.md` "Future
  Consolidation". One cargo/npm hydration instead of three. Schedule a dedicated refactor PR.

---

## 6. Do NOT Cut

| Workflow | Reason |
|---|---|
| `adversarial-tests.yml` | Determinism gate (runs twice and diffs). Loss regresses consensus safety. |
| `rust-shared-native-parity.yml` | Enforces shared/native parity invariants — core protocol gate. |
| `tdd-phase3c.yml` | Active TDD gate for Phase 3C quality-slashing; PR-gating logic lives here. |
| `ci-readiness.yml` tests job | Primary correctness signal remaining after Feb 2026 consolidation. |
| `release-signing.yml` Linux cells | ubuntu-22.04 cells are cheap and are the canonical distributed binaries. |
| `branch-protection-setup.yml` | Configuration only; fires once per file change. |

---

## 7. Structural Recommendations

None implemented in this pass.

1. **Concurrency groups** on all workflows (copy from `sdks-extensions-javascript.yml`).
2. **Path filters** on any PR-triggered workflow. If/when PR triggers return to `ci-readiness.yml`,
   split paths so a pure docs PR doesn't pay for cargo rebuilds.
3. **Shared Rust cache layer**: standardize on `Swatinem/rust-cache@v2`. Today `ci-readiness.yml`,
   `adversarial-tests.yml`, `tdd-phase3c.yml`, `rust-shared-native-parity.yml`, `sbom-generate.yml`,
   and `release-signing.yml` each use hand-rolled `actions/cache@v5` with different keys.
4. **`cargo-binstall` for tools** (§4.2).
5. **Rust toolchain pin standardization** — `WORKFLOW_JUSTIFICATION.md` flags this. Some workflows
   pin 1.70 / 1.75.0 / 1.88.0 / 1.89.0. Standardize on 1.88.0 for cache reuse.
6. **`if: github.repository_owner == 'wrfcoin'`** on cron workflows to prevent burn on forks.

---

## 8. Estimated Savings (back-of-envelope only)

| Item | Estimated monthly savings |
|---|---|
| 4.1 concurrency groups | 2–5% of scheduled-workflow minutes |
| 4.2 cached tool binaries | 15–30 min/month on cache-miss cycles |
| 4.3 Dependabot concurrency | 5–15% of total CI (workflow is ~75% of runs) |
| 4.4 docker matrix cap | Upper-bound protection only, no immediate savings |
| 5.1–5.2 release matrix cuts | 150–400 min per release event (rare) |
| 5.3 CI-readiness parallelization | ~50% wall-clock on the tests job |
| 5.4 security workflow merge | 5–10 min per weekly cycle |

**Gross rough estimate:** All low-risk cuts land → **10–20%** monthly savings on top of the
March-16 weekly-schedule cut. Medium-risk cuts add another 5–10% on amortized months.

**Confidence: low.** Real numbers require billing portal minutes.

---

## 9. Follow-up Questions (need billing portal access)

1. Actual monthly Actions minute budget for the Teams plan on `wrfcoin/core4`?
2. Per-workflow minutes consumed in the last 30 days?
3. Does the weekly stagger fit in the budget, or is there still pressure?
4. Do cached-step minutes count the same as uncached?
5. Top runners by absolute minutes (ubuntu-latest vs macos-14 vs windows-2022)?
6. Is `dependabot-auto-merge.yml`'s `wait-on-check-action` polling billed as wall-clock runner
   minutes or as webhook-tier?
7. Do forks still bill the parent account under the Teams plan?
8. Do sibling issues in backend / frontend / infra repos need the same triage pass?

---

Refs #745
