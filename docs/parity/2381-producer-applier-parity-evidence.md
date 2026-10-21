# #2381 — Producer/Applier Parity & State-Root Convergence (evidence log)

**Persona:** Bob (Opus) · **Branch:** `bob/2381-parity-epic` · **Base:** `origin/main` @ `a6595dbae`
**Context:** #2384 (`6d75fb186`) restored the `launch_api` test target that was fully
dark, so the producer/applier parity suite compiles for the first time. The prior
"parity 8/9" verdict was **vacuous** — the suite never ran. This epic RUNS it for real.

## North star

Tomorrow's genesis cut succeeds **iff two nodes converge on the same `state_root`**.
This log proves that property is load-bearing on current `main`, not assumed.

## Queue

1. **#2381** — RUN producer/applier parity for real; resolve the attested-reward case
   (real divergence vs never-ran artifact); prove the suite is load-bearing (RED-on-revert).
2. **#2383** — root-cause `earn_loop_oracle_gate_e2e` 2/2 failure.
3. Triage the newly-unmasked native-chain tests (#2384 restored a 275-test bin target).
4. Read-only review of `verify-two-node-convergence.sh`.

## Evidence

_(populated as the epic executes — every claim backed by pasted command output)_

### 1. Parity suite inventory

The producer/applier parity harness lives in `native-chain/src/bin/launch_api/p2p_events.rs`
(`mod tests`). It performs, per scenario: a serde field-by-field `State` diff **plus** a
byte-level bisect of the `state_root` pre-image, then asserts ledger (`accounts`) parity
and full `state_root` parity between the block PRODUCER path and the P2P APPLIER path.

Parity scenarios: coinbase-only, commit-reveal-lifecycle-expiry, governance-transfer,
shielded-deposit, confidential-transfer, shielded-withdrawal, reward-claim,
gamification-payout, oracle-registration, **attested-reward**, low-quality-streak,
low-quality-slash-failure, reward-and-low-quality-slash-success.

### 2. VERDICT — CONVERGES (NOT a cut blocker)

**Producer and applier converge on `state_root` for every scenario that produces a
block.** The genesis-cut property holds on `main` @ `a6595dbae`.

**First run (as-found on main), 14 parity tests:** 9 passed, 5 failed.

The 5 failures were **NOT** producer/applier divergences. Every one panicked at
**mempool admission** with `commit sender authorization failed`, at the
`.expect("mempool admits ...")` line — *before any block was mined or any parity
assertion ran*:

```
p2p_events.rs:4782  mempool admits pop tx: commit sender authorization failed
p2p_events.rs:3636  mempool admits contribution commit: commit sender authorization failed
```

Root cause: **#1848** added fail-closed commit-sender authorization at mempool
admission (`Mempool::validate_launch_payload_and_replay_key` →
`tx.verify_commit_authorization()`, `shared-protocol/src/mempool/pool.rs:946`). The
five parity fixtures build a `CommitPayload { auth_pubkey: None, auth_signature: None }`,
so admission fail-closes and the fixture panics in setup. This is **fixture drift**,
not a consensus divergence.

The 9 tests that passed on the as-found tree — coinbase-only, governance-transfer,
shielded-deposit/withdrawal, confidential-transfer, reward-claim, gamification-payout,
oracle-registration, and the field-coverage guard — do not build a `ContributionCommit`,
so #1848 never rejected them. All printed **byte-identical `state_root` pre-images** with
producer & applier stored roots matching the recompute.

**The "attested-reward 8/9 failure" is therefore an ARTIFACT of the suite never
having run (never-compiled until #2384), not a real divergence.**

### 3. Fixture fix → the reward path itself now proven convergent

Fixed the two fixture sites (`p2p_events.rs`) to sign the commit authorization the
same way the collector/backend do (`CommitPayload::sign_authorization`). The auth key
is not bound to the reward recipient (the reward follows the reveal sender;
`auth_pubkey`/`verify_authorization` are not referenced in `state.rs` or
`proof_of_publishing.rs`), so a dedicated fixture key is correct.

**After the fix: 14 passed / 0 failed.** The reward path (attested-reward), the
low-quality slash paths, and the commit-reveal lifecycle now actually mine a block and
assert parity — and CONVERGE:

```
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 321 filtered out
```

10 scenarios ran a full `state_root` pre-image comparison: **10/10 BYTE-IDENTICAL,
20/20 recompute matches, 0 divergences.**

Notably `reward+low-quality-slash-success` shows `quality_policy_events` + `audit_events`
diverging between producer (records telemetry) and applier (does not) — yet the
`state_root` pre-image is **byte-identical**. This is the #1681 design working exactly as
intended: telemetry is excluded from the consensus root, so nodes converge on `state_root`
even when their telemetry differs.

### 4. Load-bearing proof (RED-on-revert)

A parity suite that cannot fail is worthless. Injected a deliberate applier-only
divergence — `transition_extractor.rs` coinbase credit `output.amount + 1` — and ran
`producer_applier_block_apply_parity_coinbase_only`:

```
DIVERGENT FIELD: accounts
FIRST PRE-IMAGE DIFF at byte offset 57
thread '...coinbase_only' panicked: [coinbase-only] LEDGER (accounts) diverged — #1681 coinbase/reward credit regressed
test result: FAILED. 0 passed; 1 failed
```

The guard fires on a real consensus-field divergence, caught by both the serde field
diff and the byte-level pre-image bisect. Probe reverted; clean tree re-run: **14/14 green.**

**CUT VERDICT for #2381: CONVERGES. No producer/applier state_root divergence on
`a6595dbae`. Safe for the genesis cut on this axis.**

