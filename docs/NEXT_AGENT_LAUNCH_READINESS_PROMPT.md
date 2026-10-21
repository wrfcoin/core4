# Next Agent Prompt: Wrfcoin Launch Readiness (Safe + Effective)

## Mission
You are the next engineering agent responsible for making Wrfcoin launch-ready on the native chain.  
Your job is to perform a deep technical review and directly implement/fix anything necessary for a safe, fair, and operationally reliable launch.

## Required Outcome
Produce a concrete **go/no-go** recommendation backed by code changes, tests, and runbook updates.  
Do not stop at analysis. Implement high-impact fixes you find.

## Current Context
- Recent foundational work exists for:
  - Native launch API runtime (`launch_api`)
  - Commit/reveal + oracle attestation endpoints
  - Oracle quorum configuration API
  - CI workflow for native launch API integration test
  - MVP feature gating in frontend/backend
- Worktree should be clean on branch:
  - `safety/cleanup-worktree-2026-02-08`
- Recent commits to review first:
  - `7b10362fc`
  - `f10eb5836`
  - `f7ddc544b`
  - `af53ba0f0`
  - `ead91ef93`

## Working Rules
1. Treat this as a launch-critical audit + implementation task.
2. Prioritize security, consensus correctness, reward integrity, and operability.
3. For every issue found, either:
   - Fix it with tests, or
   - Document exact risk, exploit path, and mitigation if too large for one pass.
4. Keep commits clean by concern (`fix/security`, `fix/consensus`, `test`, `docs`, `chore`).
5. Do not revert unrelated user work.

## Execution Plan (Do All)

### 1. Baseline and Reproducibility
1. Verify clean build and tests from scratch.
2. Validate all launch-critical commands:
   - `cargo test -p wrfcoin-native-chain`
   - `cargo build -p wrfcoin-native-chain --bin launch_api`
   - `WRFCOIN_API_BASE_URL=http://127.0.0.1:8556 npx jest tests/integration/native-chain-launch-api.test.js --runInBand --forceExit`
   - `npm run dev:mvp` (smoke start)
3. Identify flaky/non-deterministic behavior and fix it.

### 2. Consensus and Reward Integrity Audit
Review and harden:
- `src/consensus/proof_of_publishing.rs`
- `src/blockchain/state.rs`
- `src/contributions/commit_reveal.rs`
- `src/oracle/attestation.rs`
- `src/blockchain/token_emission.rs`

Implement/fix as needed:
1. Ensure rewards mint only for valid reveal + oracle quorum.
2. Ensure unattested reveals cannot leak reward paths.
3. Ensure duplicate/replay/double-attestation and timing edge cases are enforced.
4. Ensure stake-lock routing cannot be bypassed.
5. Ensure governance parameter updates cannot create unsafe states (quorum=0, invalid windows, etc).

### 3. API Runtime Consistency and Safety
Review and harden:
- `src/api/handlers/*`
- `src/api/routes.rs`
- `native-chain/src/bin/launch_api.rs`

Implement/fix as needed:
1. Ensure handlers consistently use unified runtime/state interface.
2. Add input validation for all launch-critical payloads.
3. Normalize error responses and HTTP status codes.
4. Add missing launch endpoints if needed for observability:
   - commit lifecycle/status
   - oracle config/status
   - stake lock/spendable metrics

### 4. Security Hardening
Add targeted protections:
1. Payload-size limits and malformed JSON protection for all contribution/oracle endpoints.
2. Replay and spam resistance checks in mempool/admission.
3. Signature and oracle identity validation paths.
4. Abuse-focused tests (invalid reveal, bad attestation, stale windows, quorum mismatch).

### 5. Testing Depth Upgrade
Expand tests beyond happy path:
1. Property/edge tests for commit/reveal windows and replays.
2. Quorum threshold boundary tests (`quorum=1`, `quorum=n`, `quorum>set` rejection).
3. Reward issuance invariants under mixed transaction sets.
4. Governance update tests that mutate launch-critical params mid-flow.
5. Ensure CI runs these tests in non-skipped mode.

### 6. Launch Runbook and Operational Readiness
Update docs with exact commands and rollback steps:
- `docs/runbooks/NATIVE_CHAIN_LAUNCH_RUNBOOK.md`
- `docs/native-chain-launch-plan.md`

Add or tighten:
1. Pre-launch checklist with pass/fail gates.
2. Incident playbooks (oracle outage, bad quorum config, stuck commit queue, reward anomaly).
3. Backup/restore and chain-state validation steps.
4. Release cut procedure and post-launch monitoring checklist.

## Mandatory Deliverables
1. Code fixes committed with clear messages.
2. New/updated tests proving critical invariants.
3. Updated runbook and launch plan with any changed behavior.
4. Final report in markdown with:
   - Critical findings fixed
   - Medium/low risks remaining
   - Exact files changed
   - Test evidence
   - Final go/no-go recommendation

## Acceptance Criteria
1. All launch-critical tests pass consistently.
2. No known path to mint rewards without valid reveal + quorum.
3. No known path to bypass earned-stake lock.
4. API/runtime behavior is consistent and observable.
5. Runbook is executable end-to-end by another engineer.

## Start Here
1. Read latest launch docs and recent commits.
2. Run baseline commands.
3. Open highest-risk modules listed above.
4. Fix first, then document.
