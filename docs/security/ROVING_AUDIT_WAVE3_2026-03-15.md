# Roving Security Audit — Wave 3

**Date:** 2026-03-15
**Scope:** All 11 active repos in wrfcoin workspace (281,700+ LOC Rust, plus TypeScript, Solidity, Dart)
**Auditor:** Lane 4 automated audit (Claude Code)
**Findings:** 29 new issues filed (deduplicated against 103 pre-existing open issues)

## Executive Summary

Cross-repo security sweep targeting unwrap() panics, unsafe casts, TODO stubs, eval/exec injection,
unchecked arithmetic, O(n^2) patterns, unbounded collections, and cross-repo type mismatches.

| Priority | Category | Count |
|----------|----------|-------|
| P1 | Financial Loss | 7 |
| P2 | Consensus Split | 4 |
| P3 | Denial of Service | 4 |
| P4 | Logic Bugs | 6 |
| P5 | Code Quality | 8 |
| **Total** | | **29** |

| Repo | Issues Filed |
|------|-------------|
| core4 | 15 |
| backend | 8 |
| smart-contracts | 4 |
| mobile | 2 |

## Findings

### Priority 1: Financial Loss

| # | Repo | Issue | Title |
|---|------|-------|-------|
| 1 | core4 | [#696](https://github.com/wrfcoin/core4/issues/696) | 43 unwrap() in reward distribution — consensus panic during finality |
| 2 | core4 | [#697](https://github.com/wrfcoin/core4/issues/697) | Balance arithmetic unchecked += — integer overflow corrupts balances |
| 3 | core4 | [#698](https://github.com/wrfcoin/core4/issues/698) | Quadratic voting f64->u64 cast precision loss — biases large stakers |
| 4 | smart-contracts | [#60](https://github.com/wrfcoin/smart-contracts/issues/60) | YieldFarmingManager._getPoolConfig() always reverts — farm creation broken |
| 5 | smart-contracts | [#61](https://github.com/wrfcoin/smart-contracts/issues/61) | CommunityGovernance .transfer() breaks for contract recipients (2300 gas) |
| 6 | smart-contracts | [#62](https://github.com/wrfcoin/smart-contracts/issues/62) | Unvalidated arbitrary .call() — governance can execute anything |
| 7 | backend | [#208](https://github.com/wrfcoin/backend/issues/208) | Hardcoded fallback encryption key 'wrfcoin-dev-encryption-key' |

### Priority 2: Consensus Split

| # | Repo | Issue | Title |
|---|------|-------|-------|
| 8 | core4 | [#699](https://github.com/wrfcoin/core4/issues/699) | panic!("invalid reveal type") crashes consensus node |
| 9 | core4 | [#700](https://github.com/wrfcoin/core4/issues/700) | ed25519-dalek version mismatch 2.0 vs 2.1 across workspace |
| 10 | core4 | [#701](https://github.com/wrfcoin/core4/issues/701) | 7 unsafe FFI blocks with .to_str().unwrap() crash on malformed C data |
| 11 | core4 | [#702](https://github.com/wrfcoin/core4/issues/702) | Hex decode unwrap() panics on invalid state transitions |

### Priority 3: Denial of Service

| # | Repo | Issue | Title |
|---|------|-------|-------|
| 12 | core4 | [#703](https://github.com/wrfcoin/core4/issues/703) | O(n*m) filtering in commit-reveal validation |
| 13 | core4 | [#704](https://github.com/wrfcoin/core4/issues/704) | O(n*m) DAG blue set intersection bottleneck |
| 14 | backend | [#209](https://github.com/wrfcoin/backend/issues/209) | Emergency declareEmergency endpoint has no auth |
| 15 | core4 | [#705](https://github.com/wrfcoin/core4/issues/705) | Snapshot route serializes all accounts without pagination |

### Priority 4: Logic Bugs

| # | Repo | Issue | Title |
|---|------|-------|-------|
| 16 | backend | [#210](https://github.com/wrfcoin/backend/issues/210) | Float currency in routes vs core4 u64 base units — rounding errors |
| 17 | smart-contracts | [#63](https://github.com/wrfcoin/smart-contracts/issues/63) | CarbonCreditRouter doesn't validate pool is a CarbonCreditAMM |
| 18 | backend | [#211](https://github.com/wrfcoin/backend/issues/211) | FFI library loaded without path validation |
| 19 | backend | [#212](https://github.com/wrfcoin/backend/issues/212) | SQL injection detector exists but never wired into query layer |
| 20 | backend | [#213](https://github.com/wrfcoin/backend/issues/213) | Path traversal — route-durable-store.ts concatenates paths unsanitized |
| 21 | backend | [#214](https://github.com/wrfcoin/backend/issues/214) | fileValidation.ts eval/Function patterns — code injection risk |

### Priority 5: Code Quality / Optimization

| # | Repo | Issue | Title |
|---|------|-------|-------|
| 22 | core4 | [#706](https://github.com/wrfcoin/core4/issues/706) | CI gate: clippy lint to disallow new unwrap() in consensus/blockchain |
| 23 | core4 | [#707](https://github.com/wrfcoin/core4/issues/707) | Vec::new() without capacity hints in consensus hot paths |
| 24 | core4 | [#708](https://github.com/wrfcoin/core4/issues/708) | 22 unwrap() in ring_signatures.rs — privacy circuit panics |
| 25 | core4 | [#709](https://github.com/wrfcoin/core4/issues/709) | 13 TODO stubs in collaborative_pools.rs reward distribution |
| 26 | core4 | [#710](https://github.com/wrfcoin/core4/issues/710) | Invariant modules (balance.rs, slashing.rs) unwrap on mutations |
| 27 | mobile | [#64](https://github.com/wrfcoin/mobile/issues/64) | No SSL/TLS certificate pinning in Dio client — MITM vector |
| 28 | mobile | [#65](https://github.com/wrfcoin/mobile/issues/65) | Hardcoded localhost:4001 API URL in production builds |
| 29 | backend | [#215](https://github.com/wrfcoin/backend/issues/215) | Emergency notification delivery not implemented — logs only |

## Cross-Repo Impact Matrix

| Finding | core4 | backend | smart-contracts | mobile |
|---------|-------|---------|-----------------|--------|
| Currency type mismatch (float vs u64) | X | X | | |
| Encryption key fallback | | X | | |
| Governance execution safety | | | X | |
| Certificate/TLS gaps | | | | X |
| unwrap() panics (systemic) | X | | | |

## Remediation Priority

**Immediate (this sprint):**
- #208 (backend): Remove hardcoded encryption key fallback
- #696 (core4): Replace 43 unwrap() in reward distribution
- #699 (core4): Replace panic!() with error propagation in verify_block
- #60 (smart-contracts): Implement _getPoolConfig()

**Next sprint:**
- #697, #702 (core4): Checked arithmetic + hex decode error handling
- #61, #62 (smart-contracts): .transfer() -> .call() + governance whitelist
- #209 (backend): Auth on emergency endpoints

**Backlog:**
- All P4 logic bugs and P5 code quality issues

## Methodology

1. Automated grep-based scan of 665 Rust files, 200+ TypeScript files, 348+ Solidity contracts
2. Pattern matching for: `unwrap()`, `unsafe`, `as u32/u64/i64/usize`, `TODO/FIXME/HACK`,
   `panic!/unreachable!/unimplemented!`, `eval/exec/Function`, unchecked arithmetic operators,
   nested loops with `.contains()`, unbounded `push/insert`, hardcoded secrets
3. Manual review of top-risk modules (consensus, state, rewards, governance)
4. Cross-reference against 103 existing open issues to deduplicate
5. Priority assignment: financial loss > consensus split > DoS > logic bugs > optimization
