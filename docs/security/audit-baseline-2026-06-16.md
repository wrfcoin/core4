# Supply-Chain Audit Baseline — 2026-06-16

**Owner:** Dan (security-engineering, supply-chain reconciliation slice of epic #1701)
**Branch:** `docs/1701-cve-acceptance-register`
**Scope:** Establish the current, reproducible advisory baseline for both ecosystems
(Cargo + npm) at the launch freeze, and reconcile the two weekly supply-chain scan
issues (#1557 → superseded by #1701). Companion document:
[`cve-acceptance-register.md`](./cve-acceptance-register.md) (the canonical
cross-cutting disposition index).

> **Docs-only.** No `Cargo.lock` / `package.json` / source changes in this PR. The
> actual dependency bumps land in the parallel Rust (#1470–#1474, #1591) and npm
> (#1585–#1590) remediation lanes; this baseline is the measurement they are graded
> against.

---

## 1. Tooling note — why the numbers differ from the CI scan

The weekly CI scan in #1557/#1701 uses **`osv-scanner`** (OSV + GitHub Advisory
database, GHSA-keyed). This baseline additionally records **`cargo audit`**
(RustSec advisory database) and **`npm audit`** (npm registry advisory database).

These tools draw from **different advisory databases and version-range
interpretations**, so the counts are not expected to match 1:1. The most important
divergences, all reconciled below:

| Crate | osv-scanner (CI, #1557/#1701) | `cargo audit` (RustSec, local) | Reconciliation |
|-------|-------------------------------|--------------------------------|----------------|
| `openssl 0.10.78` | flagged: GHSA-xp3w-r5p5-63rr (8.7), GHSA-xv59-967r-8726 (5.1), GHSA-phqj-4mhp-q6mq (5.1) | **not flagged** | The RustSec openssl advisories fix-line is below 0.10.78; the safe openssl bump already landed in **PR #1468 (merged 2026-04-27)**. RustSec considers 0.10.78 clean. The GHSA rows are either stale ranges in the 05-11/06-01 snapshots or osv version-range over-matching. **Treat as remediated / verify.** |
| `yamux 0.12.1` | flagged: GHSA-vxx9-2994-q338 (8.7) | **not flagged** | GHSA-only advisory not mirrored to RustSec. **The crate is genuinely present and vulnerable** (see §3). cargo audit's silence is a DB gap, *not* an all-clear. |
| `actix-http 3.12.0` | flagged: GHSA-xhj4-vrgc-hr34 (6.3) | **not flagged** | GHSA-only request-smuggling advisory. Real; patched release 3.12.1 now exists (see register / #1473). |

**Takeaway:** neither tool is a superset of the other. The launch baseline is the
**union** of osv-scanner + cargo audit + npm audit, captured in the register.

---

## 2. Rust baseline (`cargo audit`, RustSec DB)

Run in the worktree against the current `Cargo.lock` (737 packages):

```
cargo audit            # cargo-audit 0.22.1
→ error: 3 vulnerabilities found!
→ warning: 7 allowed warnings found
```

### Vulnerabilities (3)

| Advisory | Crate | Ver | Severity | Patched | Disposition (see register) |
|----------|-------|-----|----------|---------|----------------------------|
| RUSTSEC-2026-0118 | hickory-proto | 0.25.2 | 8.7 | *(none yet)* | track (#1591) |
| RUSTSEC-2026-0119 | hickory-proto | 0.25.2 | 6.9 | ≥0.26.1 | track (#1591) |
| RUSTSEC-2025-0055 | tracing-subscriber | 0.2.25 | low (2.3) | ≥0.3.20 | deferred — ark 0.5 (#1472) |

### Warnings (7: 5 unmaintained, 1 unsound, 1 yanked)

| Advisory | Crate | Ver | Class | Disposition |
|----------|-------|-----|-------|-------------|
| RUSTSEC-2025-0141 | bincode | 1.3.3 | unmaintained | deferred — 1→2 migration (#1470) |
| RUSTSEC-2026-0105 | core2 | 0.4.0 | unmaintained + **yanked** | deferred — libp2p ecosystem (#1471) |
| RUSTSEC-2024-0388 | derivative | 2.2.0 | unmaintained | deferred — ark 0.5 (#1472) |
| RUSTSEC-2024-0436 | paste | 1.0.15 | unmaintained | deferred — ark 0.5 (#1472) |
| RUSTSEC-2025-0119 | number_prefix | 0.4.0 | unmaintained | accepted — CLI-only (#1474) |
| RUSTSEC-2026-0002 | lru | 0.12.5 | unsound (IterMut) | accepted — CLI-only, path unused (#1474) |

*(The 7th warning is the `core2` **yanked** notice, same crate as RUSTSEC-2026-0105.)*

**GHSA-only Rust advisories not surfaced by RustSec but tracked anyway** (from the
osv-scanner CI rows): `actix-http` GHSA-xhj4-vrgc-hr34 (#1473), `yamux 0.12.1`
GHSA-vxx9-2994-q338 (#1471), `openssl 0.10.78` GHSA-xp3w/xv59/phqj (verify —
RustSec-clean, #1468 landed the bump). See register for full disposition.

---

## 3. Dependency-path verifications (`cargo tree -i`)

| Crate | In tree? | Path | Note |
|-------|----------|------|------|
| `yamux 0.12.1` | **YES** | `libp2p-yamux 0.47.0 → libp2p 0.56.0 → wrfcoin-p2p` | **Not moot.** Both `yamux 0.12.1` and `0.13.10` exist in the lock; 0.12.1 is live. Prior survey note ("likely moot") is **corrected**: advisory is real, fix requires a libp2p 0.57+ bump (#1471). |
| `hickory-proto 0.25.2` | YES | `hickory-resolver 0.25.2 → libp2p-dns 0.44.0 → libp2p 0.56.0 → wrfcoin-p2p` | 0.25.3+ pinned out by libp2p 0.56; bump blocked until libp2p moves (#1591/#1471). |
| `actix-http 3.12.0` | YES | `actix-web 4.13.0 → actix-cors/-web-actors → wrfcoin-native-chain` | launch_api HTTP surface; 3.12.1 patch available (#1473). |
| `openssl 0.10.78` | YES | `native-tls → reqwest 0.12.28 → (execution-engine, shared-protocol, sdk, cli)` | RustSec-clean at 0.10.78 (post-#1468). |
| `bincode 1.3.3` | YES | direct dep of `native-chain` + `p2p` (wire + WAL/state) | breaking 1→2; consensus/persistence-sensitive (#1470). |

**Duplicate libp2p trees:** the lock carries **two** libp2p majors — `0.54.1`
(pulls `lru 0.12.5` via `libp2p-swarm 0.45.1`) and `0.56.0` (pulls
`hickory-proto`, `yamux 0.12.1`, `core2`). The libp2p-ecosystem bump (#1471)
should unify these, which would clear yamux + core2 + (with a minor) hickory-proto
together.

---

## 4. npm baseline (`npm audit`, npm registry DB)

Run at the repo root against `package-lock.json` (3051 packages):

```
npm audit --json   # npm 10.8.2 / node v20.19.6
```

### Severity counts (root, all workspaces, incl. dev)

| Severity | Count |
|----------|-------|
| Critical | **3** |
| High | **28** |
| Moderate | **129** |
| Low | **39** |
| **Total** | **199** |

*(`npm audit` counts vulnerable **packages/advisory-chains**; osv-scanner counts
**advisory rows per package-version**. The #1557 prod-only slice was reported as
31 prod vulns / 7L-13M-10H-1C; this root figure includes dev + all workspaces and
is the broader number. Both are valid under their respective tools.)*

### Critical advisories (3)

| Package | Range | Owning issue | Headline |
|---------|-------|--------------|----------|
| `protobufjs` | ≤7.6.2 | **#1585** | Arbitrary code execution / prototype pollution (RCE) — **highest priority npm item** |
| `shell-quote` | 1.1.0–1.8.3 | #1590 (non-major bundle) | `quote()` does not escape newlines in object `.op` values |
| `concurrently` | 9.2.1 | #1590 (non-major bundle) | inherits the `shell-quote` chain (dev tooling) |

### High advisories (28 packages) — cluster ownership

Mapped to the npm remediation lanes (Bob's slice). Representative members:

- **#1586 axios** — `axios 1.0.0–1.15.2` (SSRF / DoS / data-leak chain).
- **#1587 hono / @hono/node-server** — body-limit / cookie chain (moderate-heavy).
- **#1588 task-master-ai major** — unblocks `@google/gemini-cli-core`,
  `ai-sdk-provider-gemini-cli`, `@opentelemetry/sdk-node`,
  `@opentelemetry/exporter-prometheus`, `@grpc/grpc-js`, `basic-ftp`, `fast-uri`
  (semver-major; needs functional validation).
- **#1589 ip-address / geoip-lite** — XSS chain.
- **#1590 non-major bundle** — `esbuild`/`vite`/`tsx`/`@esbuild-kit/*`, `ws`,
  `tmp`, `form-data`, `simple-git`, `patch-package`, `fast-xml-builder`,
  `systeminformation`, plus the two criticals above.
- **Solidity dev-deps** (not launch-runtime): `@openzeppelin/contracts 4.x`,
  `hardhat`, `solc`, `@chainlink/contracts`, `@arbitrum/nitro-contracts`,
  `react-router(-dom)`, `drizzle-kit` — dev/build scoped; tracked but lower
  launch-blast-radius. (OZ contract advisories also handled in the
  smart-contracts repo lane.)

Full per-package npm dispositions are owned by Bob (#1585–#1590) and populated
into the register's npm section as those PRs land.

---

## 5. Reconciliation: #1557 vs #1701

See [`cve-acceptance-register.md`](./cve-acceptance-register.md) §"Issue
reconciliation" for the full mapping. Summary:

- **#1557** ([Security] …2026-05-11) and **#1701** ([Security] …2026-06-01) are
  two weekly snapshots of the **same** root `package-lock.json` + `Cargo.lock`.
- **Rust:** #1557's advisory set is a **strict subset** of #1701's. #1701 adds
  exactly one row (`openssl 0.10.78` GHSA-phqj-4mhp-q6mq, 5.1). #1557 has **zero
  unique Rust advisories**.
- **npm:** the surfaces overlap; individual GHSA rows rotate between snapshots as
  the advisory DB gains entries, but **both issues are remediated by the identical
  cluster set** (#1585–#1591 + #1470–#1474). #1557 is where those cluster issues
  were *created*; #1701 *adopted* them as its tracked sub-stories (and explicitly
  lists #1557 itself as a story).
- **Verdict:** #1557 is **superseded by #1701** — recommend close as superseded.
  No remediation work is lost: every #1557 follow-up is already a #1701 sub-issue.

---

## 6. Reproduction

```bash
# Rust
cargo audit --json          # RustSec DB
cargo tree -i yamux@0.12.1   # verify live paths (disambiguate duplicate versions)

# npm (repo root)
npm audit --json            # npm registry DB

# CI parity (OSV/GHSA) — as run by the weekly scan
# osv-scanner --lockfile=Cargo.lock --lockfile=package-lock.json
```
