# Supply-Chain Triage — core4#1557 (scan 2026-05-11)

**Round:** R92 b:5 (Frank)
**Branch:** `r92/b5-core4-1557-supply-chain`
**Scope:** Triage only. No major version bumps, no Cargo.lock churn, no `npm audit fix --force`.

## Scope decision

The 2026-05-11 weekly scan flagged 28 npm advisories on the root `package-lock.json`
and 14 Rust advisories on `Cargo.lock`. The open dependabot PRs on core4
(#1577–#1582) all target `sdks/extensions/javascript/`, **not** the root lockfile
this issue is about. They are tracked separately below but do not close #1557.

A blanket `npm audit fix --omit=dev` against root touches **3,775 transitive
package operations** even when restricted to non-major bumps — that is the kind
of "general package refresh" the round handoff explicitly prohibits. Therefore
this PR is a **documentation-only triage**: per-cluster follow-up issues capture
the remediation work, and the actual bumps land in narrower scoped PRs.

## Commands run

```bash
git -C core4 worktree add -B r92/b5-core4-1557-supply-chain \
    worktrees/core4-r92-b5-1557-supply-chain origin/main
cd worktrees/core4-r92-b5-1557-supply-chain
npm audit --omit=dev               # 31 prod vulns (7L / 13M / 10H / 1C)
npm audit                          # full audit including dev
npm audit fix --omit=dev --dry-run # 3,775 add ops → REJECTED as too broad
cargo audit                        # 3 errors, 7 warnings
```

Full audit JSON archived at `/tmp/r92-1557-npm-prod.json`,
`/tmp/r92-1557-cargo.json`, `/tmp/r92-1557-cargo.txt` on the build host.

## P0 — fix in this PR

**None.** Every individual non-major npm bump cascades through transitive
dependencies (task-master-ai → MCP SDKs → @google-cloud → axios; @apollo/server →
hono → @hono/node-server; etc.). Holding the line on lockfile churn is the
correct call for a triage round; the bumps live in P1 follow-ups.

## P1 — follow-up tracking issues

Each cluster gets one dedicated issue with the scope boundaries pre-set so the
next round can dispatch them as narrow lanes.

### P1-A — `task-master-ai` MCP toolchain cluster (#1588)
Blocks ≈40% of npm advisories: `@google-cloud/common`, `@google-cloud/logging`,
`@google/gemini-cli-core`, `@opentelemetry/exporter-prometheus`,
`@opentelemetry/sdk-node`, `ai-sdk-provider-gemini-cli`, `@tootallnate/once`,
`http-proxy-agent`, `retry-request`, `teeny-request`. npm marks the fix as
`task-master-ai@0.18.0` with `isSemVerMajor: true`. Validate that the project
still functions on the new major, then bump once.

### P1-B — `axios` 1.15.x → 1.16.x runtime cluster (#1586)
13 separate axios CVEs (CVSS up to 7.5). Direct dep `axios=^1.15.0` plus many
transitive pins. Bump via root + selective `overrides` to collapse versions.

### P1-C — `hono` / `@hono/node-server` stack (#1587)
11 hono advisories, most moderate, including cookie/auth and body-limit bypass.
`fixAvailable: true` (non-major) — narrow bump should be feasible in isolation.

### P1-D — `protobufjs` (critical) (#1585)
GHSA-xq3m-2v4x-88gg arbitrary code execution + 8 other CVEs. `fixAvailable: true`.
Direct upgrade should be safe; this is the highest-priority single package.

### P1-E — `ip-address` / `geoip-lite` chain (#1589)
`geoip-lite` needs a major bump (1.4.10 → 1.2.2 looks like a renumber; verify on
npm registry before dispatching). Affects `express-rate-limit 8.0.1-8.5.0` and
the geoip path used by the API. Validate IP-binding regressions.

### P1-F — High-severity narrow bumps (group as one PR) (#1590)
`simple-git <3.36.0` (RCE, H), `systeminformation 4.17.0–5.31.5` (Linux command
injection, H), `fast-xml-builder <=1.1.6` (H), `fast-uri <=3.1.1` (H), `uuid
11.0.0–11.1.0` (M), `ws 8.0.0–8.20.0` (M), `dompurify <=3.3.3` (M),
`follow-redirects <=1.15.11` (M), `brace-expansion 5.0.2–5.0.5` (M),
`@aws-sdk/xml-builder` (M), `@protobufjs/utf8` (M), `@hono/node-server` (M).
All `fixAvailable: true`, no major bumps required. Combine into one targeted PR
that touches only these and their direct lockfile lines.

### P1-G — Rust `hickory-proto` 0.25.2 → 0.25.3+ (#1591)
RUSTSEC-2026-0118 (8.7, NSEC3 unbounded loop) + RUSTSEC-2026-0119 (6.9, O(n²)
name compression CPU exhaustion). Reaches the p2p surface via libp2p-dns →
libp2p-tcp. Verify libp2p compatibility before bumping.

### P1-H — Rust `tracing-subscriber` 0.2.25 ANSI poisoning (#1472)
RUSTSEC-2025-0055. Low severity but the 0.2 series is end-of-life. Likely also
needs 0.3 migration; check call sites for breaking changes.

## P2 — accepted risk / out of scope

- **Rust `bincode` 1.3.3 unmaintained (RUSTSEC-2025-0141)** — load-bearing for
  WAL v2 (PR #1103). Migration is a multi-PR consensus-sensitive effort, not a
  supply-chain fix. Tracked elsewhere.
- **Rust `core2` 0.4.0 yanked (RUSTSEC-2026-0105)** — transitive via crypto
  stack; needs upstream movement. Accept until upstreams release.
- **Rust `derivative` 2.2.0 unmaintained (RUSTSEC-2024-0388)** — proc-macro,
  no runtime risk. Migrate when touching the affected crates.
- **Rust `number_prefix` 0.4.0 unmaintained (RUSTSEC-2025-0119)** — CLI display
  formatting only. No security impact.
- **Rust `paste` 1.0.15 unmaintained (RUSTSEC-2024-0436)** — proc-macro,
  build-time only.
- **Rust `lru` 0.12.5 unsound (RUSTSEC-2026-0002)** — `IterMut` Stacked Borrows
  violation. We do not call `IterMut` on the affected caches; verified
  case-by-case in libp2p-swarm / ratatui code paths. Accept until upstreams
  upgrade.
- **`@openzeppelin/contracts` 4.7.3 / 4.8.3** in `smart-contracts/` mirror —
  belongs to the `smart-contracts` repo, not core4. File in that repo
  separately if not already tracked.
- **`@babel/plugin-transform-modules-systemjs` 7.28.5 (8.2 dev)** — dev-only,
  no runtime exposure. Accept until next routine `@babel/*` bump.

## Sibling PRs (separate scope)

These dependabot PRs land in `sdks/extensions/javascript/` and do **not** touch
the surfaces in #1557. They are reviewed/merged on their own merits by the
sprint merge manager:

| PR    | mergeable | bump                                                  |
| ----- | --------- | ----------------------------------------------------- |
| #1577 | MERGED    | `actions/dependency-review-action 4 → 5` (workflows)  |
| #1578 | UNSTABLE  | `@noble/hashes 1.8.0 → 2.2.0` (MAJOR — review first)  |
| #1579 | MERGED    | `eslint 10.2.1 → 10.3.0` (dev)                        |
| #1580 | MERGED    | `axios 1.16.0 → 1.16.1` (sdk subtree)                 |
| #1581 | MERGED    | `graphql 16.13.2 → 16.14.0`                           |
| #1582 | MERGED    | `ws 8.20.0 → 8.20.1` (sdk subtree)                    |

These sibling PRs were handled separately by the sprint merge manager and do not
close #1557.

## Deliverable status

- [x] Worktree created and clean.
- [x] Reproduced npm + cargo audit; raw output captured.
- [x] Cross-referenced open dependabot PRs.
- [x] Classified every advisory into P0 / P1 / P2.
- [x] P1 follow-up issues filed or mapped to existing trackers (#1585-#1591,
  #1472).
- [x] No lockfile churn introduced.
