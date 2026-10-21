# WRFCoin Emergency Runbook

Story #2101, epic #2093. The operational half of the security model: what to do
when something goes wrong, and **who is allowed to do it**.

> **Scope.** This document answers *who decides* and *what the limits are*. It
> deliberately does **not** repeat command-level recovery steps that already
> exist elsewhere — every procedure below links to the operational runbook that
> carries the commands. Duplicated runbooks drift, and a stale copy read during
> an incident is worse than no copy.
>
> | For | Go to |
> |---|---|
> | Node down, crash-loop, peer loss, block-production stall, divergence commands | [`docs/operations/TESTNET_INCIDENT_RESPONSE_RUNBOOK.md`](../operations/TESTNET_INCIDENT_RESPONSE_RUNBOOK.md) |
> | Bridge auto-halt triggers, exposure caps, resume procedure, challenge windows | [`docs/BRIDGE_SAFETY_RUNBOOK.md`](../BRIDGE_SAFETY_RUNBOOK.md) |
> | Secret rotation mechanics | [`docs/security/SECRET-ROTATION-RUNBOOK.md`](SECRET-ROTATION-RUNBOOK.md) |
> | Backup/restore, RTO/RPO — **database/application only, not chain state** (see dead-end table below) | [`docs/security/DISASTER_RECOVERY.md`](DISASTER_RECOVERY.md) |
> | Severity classification, comms templates, post-mortem | [`docs/security/INCIDENT_RESPONSE_PLAN.md`](INCIDENT_RESPONSE_PLAN.md) |
> | Adversary classes and privileged capabilities | [`docs/security/THREAT_MODEL.md`](THREAT_MODEL.md) |
> | Normative protocol behaviour | [`docs/security/PROTOCOL_SPEC.md`](PROTOCOL_SPEC.md) |

---

## What this runbook cannot yet do — read this before an incident

Every command, path and symbol below was verified against the tree on 2026-07-22
(references marked ✅ exist and were checked; ⛔ marks a dead end; re-audited
later the same day after #2260/#2230 merged). Two procedures reach steps that
are **not executable today**. Hitting one of these during an incident is
expected; the escalation is the procedure.

| Dead end | Where | Status | Tracked by |
|---|---|---|---|
| ⛔ **Live signing-key revocation is impossible.** The only implemented caller of `RevokeSigningKeyEpoch` is `POST /transactions/signing-keys/revoke`, which requires `WRFCOIN_ENABLE_ORACLE_ADMIN_API=1` **and refuses every chain at height > 0** (`LIVE_NETWORK_MUTATION_REJECTED`, because out-of-band mutation forks the state root, #1775). No transaction/governance path applies this transition on a live network. | §5 Oracle, §7 | Not executable | **No tracking issue** — reported to manager |
| ⛔ **No chain rollback/restore tooling exists.** `DISASTER_RECOVERY.md` is database/application recovery and its commands reference scripts absent from this repo (`scripts/database-backup-restore.js`, `scripts/bridge-manager.js`, `security/hsm-secure-implementation.js`). There is no operator-invocable chain rollback or snapshot-restore command. `Blockchain::plan_reorg` (✅ exists) is the *automatic* fork-choice path, not a tool an operator can point at a height. | §3 | Not executable | **No tracking issue** — reported to manager |
| ✅ ~~The resume-gate invariant check is not merged~~ **Resolved 2026-07-22:** PR #2260 merged; `InvariantHarness::check_all` exists (`shared-protocol/src/invariants/harness.rs`) and §Resuming step 5 is executable via `cargo test -p wrfcoin-shared-protocol --test invariant_harness`. | §Resuming | Executable | — |
| ✅ ~~`docs/zk/PROVING_COSTS.md` does not exist yet~~ **Resolved 2026-07-22:** PR #2230 merged; the file exists and §5's figures now cite a document on `main`. | §5 ZK | Resolved | — |

What **is** executable today, verified: the §1 halt command (full `curl` now
included below), `GET /node/partition-status`, `GET /readyz`,
`WRFCOIN_STATE_DIVERGENCE_RECOVERY` modes (`native-chain/src/p2p_divergence.rs`),
the `INV-FINALITY-1` finality invariant (merged in #2246 and wired into the
finality consumer, `native-chain/src/bin/launch_api/finality_events.rs`),
`MAX_REORG_DEPTH`/`plan_reorg`, `.github/workflows/release-signing.yml`, and
sibling-repo `infra/scripts/verify-two-node-convergence.sh`.

---

## Authority matrix

Every action that can change consensus, custody or trust. **Read the quorum
column before anything else.**

| Action | Authorizing key / role | Quorum required | Audit trail |
|---|---|---|---|
| Halt block production on one node | `WRFCOIN_OPERATOR_TOKEN` (`ApiRole::Operator`) | **1** — single bearer token | Node logs; `wrfcoin_chain_*` metrics |
| Set partition mode (`POST /operator/partition-mode`) | `WRFCOIN_OPERATOR_TOKEN` | **1** | Node logs; `GET /node/partition-status` |
| Purge / clear mempool | `WRFCOIN_OPERATOR_TOKEN` | **1** | Node logs |
| Bridge halt / resume / adjust caps | `WRFCOIN_OPERATOR_TOKEN` | **1** | Bridge audit trail — see BRIDGE_SAFETY_RUNBOOK §Audit Trail |
| Governance proposal lifecycle | `WRFCOIN_OPERATOR_TOKEN` | **1** | `State.governance.proposals`; `audit_events` |
| Mutate oracle set / config | `WRFCOIN_OPERATOR_TOKEN` **and** `WRFCOIN_ENABLE_ORACLE_ADMIN_API=1` | **1** + env flag | `State.oracle_set`; `audit_events` |
| Register / revoke signing-key epochs | `WRFCOIN_OPERATOR_TOKEN` + admin-API flag | **1** + env flag | `State.signing_keys` (`SigningRole`: Oracle, Calibrator, BridgeOperator, GovernanceOperator, Validator) |
| Seed balances (`/debug/seed-account`) | `WRFCOIN_OPERATOR_TOKEN` **and** `WRFCOIN_ENABLE_DEBUG_SEED_API=1` | **1** + env flag | `audit_events`; **forks the state root** (#1811) |
| Disable all API auth | `WRFCOIN_ALLOW_NO_AUTH=1` | **env only** | Deployment config; node startup logs |
| Accept a state snapshot during sync | `WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS` | **env only** | Node logs; snapshot signature records |
| Submit oracle attestation | `WRFCOIN_ORACLE_TOKEN` + oracle-set membership | oracle quorum (`State.oracle_quorum`) | `State.attestations` |
| Submit contribution commit/reveal | `WRFCOIN_CONTRIBUTOR_TOKEN` | **1** | `State.pending_commits` / `pending_reveals` |
| Sign and publish a release | GitHub repo secrets, `.github/workflows/release-signing.yml` | **1** CI identity | GitHub Actions run log; release signatures |
| Deploy to testnet | SSH to the N5 host; local build only | **1** | Deploy logs; `docker compose` history |

### The two things to notice

1. **Almost every row says quorum 1.** Every privileged HTTP capability rests on
   a *single bearer token per role*. There is no multi-signature, no threshold
   scheme and no hardware second factor at the API layer. Compromise of
   `WRFCOIN_OPERATOR_TOKEN` is total compromise of that node. This is a known,
   accepted testnet posture — see THREAT_MODEL.md §Privileged capabilities — and
   it is **not** an acceptable mainnet posture. Treat every "authorize the
   operator to…" instruction below as "one person with one token can do this
   unilaterally, including by mistake."

2. **Three rows fork the state root.** Oracle-set mutation (#1775), signing-key
   epoch changes and `/debug/seed-account` (#1811) mutate state outside block
   application. Using any of them on one node of a multi-node network **causes a
   permanent divergence**, not a recoverable error. They are flag-gated off by
   default for exactly this reason. During an incident, reaching for them is
   almost always the wrong move — see §3's limits.

---

## 1. Emergency halt

**Trigger** — any of:

- A safety violation from the finality invariant: log line
  `FATAL: INV-FINALITY-1 violated: conflicting finalization at height …`
  (`shared-protocol/src/invariants/finality.rs`, #2096). The BFT finality
  consumer halts itself and `finalized_height` stops advancing.
- Consensus stall: no new block for > 10 minutes with peers reachable.
  Alert: `wrfcoin_chain_bft_last_advance_timestamp_seconds` stops advancing.
- Confirmed state-root divergence that §2 could not resolve.
- Credible report of an active exploit against a privileged route.

**Authority** — Operator token holder, unilaterally (quorum 1). No approval is
required to *halt*. Approval **is** required to resume: halting is reversible
and cheap, resuming on a broken chain is neither. **Bias to halting.**

**Steps**

1. Record evidence *before* touching anything: current height, tip hash,
   `state_root`, `finalized_height`, and the last 500 log lines from every node.
   A halt destroys the live evidence you will need for the post-mortem.
2. Set partition mode read-only on each node. This stops local block production
   without stopping the process, so diagnostics stay available. The full
   command (✅ verified against `routes/network.rs::set_partition_mode` — it
   requires an Operator bearer token and a JSON body; note
   `TESTNET_INCIDENT_RESPONSE_RUNBOOK.md` does **not** carry this command, this
   is the authoritative copy):

   ```bash
   curl -s -X POST "http://<node-host>:8556/operator/partition-mode" \
     -H "Authorization: Bearer ${WRFCOIN_OPERATOR_TOKEN}" \
     -H "Content-Type: application/json" \
     -d '{"mode":"ReadOnly"}'
   # Valid modes: "Normal", "BlockProductionOnly", "ReadOnly".
   # Success echoes {"mode":"read_only","allows_block_production":false,...}
   ```
3. If the trigger was the finality invariant, **do not restart the node to clear
   the halt.** The invariant's memory is per-process; restarting erases the
   record of the conflicting finalization and the node will happily resume
   finalizing. Preserve the process, or at minimum preserve its logs, before
   any restart.
4. Notify per §Communications.
5. Do not resume until the root cause is understood *and* a second person has
   reviewed the evidence — see §Resuming.

**Verification** — `GET /node/partition-status` reports read-only on every node;
chain height stable across two consecutive checks 60s apart; no new blocks in
peer logs.

**Comms** — Operator channel immediately; public status note within 30 minutes
if the testnet is externally attached. State what halted and that funds (which
are valueless on testnet) are unaffected.

---

## 2. Recovery from state-root divergence between peers

**Trigger** — `/readyz` reports the `state_diverged` latch, or two peers at the
same height report different `state_root`.

**Authority** — Operator token holder. Quorum 1 for the *investigation*; see the
limits below for what needs a second pair of eyes.

**Steps** — the command-level procedure is
[`TESTNET_INCIDENT_RESPONSE_RUNBOOK.md` §2 "State divergence between peers"](../operations/TESTNET_INCIDENT_RESPONSE_RUNBOOK.md).
The decisions that runbook does not make for you:

1. **Establish which node is wrong before changing anything.** The divergence
   latch is cleared according to `WRFCOIN_STATE_DIVERGENCE_RECOVERY`
   (`native-chain/src/p2p_divergence.rs`):
   - `peer-agreement` (**default**) — a same-height peer agreeing on tip hash and
     a non-zero state root clears the latch. This trusts the peer's
     *self-reported* root. A colluding or simply-wrong peer can clear the latch
     by echoing a matching root back.
   - `local-root` (stricter) — only the node's own recomputed / WAL-replay
     parity root clears it. Requires local forward progress.
2. **During an incident, prefer `local-root`.** The default exists for
   backwards compatibility, not because peer agreement is better evidence. If
   you are investigating a divergence, peer self-reports are exactly the thing
   under suspicion.
3. Check whether any state-root-forking action from the authority matrix was
   used on either node — oracle-set mutation, signing-key epoch change, or
   `/debug/seed-account`. This is the most common cause of a divergence that
   looks inexplicable, and it is confirmable from `audit_events`.

**Verification** — both nodes report identical `state_root` at the same height,
verified by `infra`'s `verify-two-node-convergence.sh` rather than by comparing
block hashes. Block-hash equality is not sufficient evidence of convergence.

**Comms** — Operator channel. Escalate publicly only if the divergence
persists past one recovery attempt.

---

## 3. Rollback

**Trigger** — a bad block or bad state must be undone.

**Authority** — Operator token holder, **plus a second reviewer**. This is the
one procedure in this document where quorum 1 is not acceptable practice even
though it is all the system enforces. A rollback is destructive and, past the
limits below, irreversible.

### What can be rolled back

| | Limit | Enforced by |
|---|---|---|
| Reorg depth | **100 blocks** (`MAX_REORG_DEPTH`) | `Blockchain::plan_reorg` refuses deeper forks |
| Below finalized height | **Never** | `plan_reorg` refuses any fork at or below `finalized_height` |
| Restore from snapshot | ⛔ **No implemented operator procedure** — see the dead-end table at the top | — |

### What CANNOT be recovered — read before starting

- **Anything at or below `finalized_height`.** BFT finality is a promise, not a
  preference. If the bad state is finalized, rollback is not the tool — that is
  a hard-fork decision, not an operational one.
- **More than 100 blocks of history** without a full restore from snapshot plus
  replay. Beyond `MAX_REORG_DEPTH` the in-memory chain no longer holds the fork
  point.
- **State mutated outside block application.** `/debug/seed-account`,
  oracle-set mutations and signing-key epoch changes do not appear in any
  block, so replaying blocks does not undo them. There is no rollback for
  these; the only remedy is a fresh genesis.
- **Data already served to external consumers.** Weather data, attestations and
  API responses that left the node cannot be recalled. Rolling back the chain
  does not roll back what integrators cached.
- **Off-chain side effects** — bridge transfers already relayed to another
  chain. Those are governed by the challenge windows in BRIDGE_SAFETY_RUNBOOK,
  not by this procedure.

**Steps**

1. Halt first (§1). Never roll back a live chain.
2. Record the target height and its hash, and confirm it is **strictly above**
   `finalized_height` on every node.
3. Confirm the depth is ≤ 100 blocks. If not, this is a restore, not a
   rollback — and ⛔ **there is currently no restore procedure to go to** (see
   the dead-end table; `DISASTER_RECOVERY.md` covers database/application
   recovery, not chain state, and its own commands reference scripts that do
   not exist in this repo).
4. Second reviewer confirms steps 2 and 3 independently, from their own copy of
   the evidence, before anything is executed.
5. ⛔ **Execution dead end.** There is no operator-invocable chain rollback
   command at this head. The in-repo reorg machinery (`plan_reorg`) acts only
   on forks the node observes via P2P; it cannot be pointed at a target height.
   What an operator can actually do today, in order of preference:
   1. Keep the network halted (§1) and escalate — this is the expected outcome
      of reaching this step, not a failure.
   2. If a healthy peer holds the desired chain, wipe the bad node's data
      volume and resync from peers/snapshot-sync (destructive to local state;
      requires `WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS` to be set correctly) — this
      is re-sync, not rollback, and only works while a correct peer exists.
   3. Past finality or past both nodes: fresh-genesis territory (#2034's
      consolidated cut is the standing precedent) — an owner decision, not an
      operational one.
6. Re-verify convergence per §2's verification before resuming production.

**Verification** — every node reports the same height, tip hash and state root;
`finalized_height` is unchanged (a rollback must never move it backwards).

**Comms** — Operator channel before starting; public note on completion stating
the height range affected.

---

## 4. Failed upgrade recovery

**Trigger** — nodes fail to start, fail to sync, or diverge immediately after a
version rollout.

**Authority** — Operator token holder. Reverting to the immediately-previous
release is unilateral. Reverting **past** it requires the check below.

**Steps**

1. Halt (§1) before rolling anything back. A half-upgraded network producing
   blocks is how a temporary problem becomes a permanent divergence.
2. Identify the last known-good release and confirm its signature per §6.
3. **Rollback protection — check this before reverting.** Confirm the target
   version is not on the known-vulnerable list in
   [`docs/security/cve-acceptance-register.md`](cve-acceptance-register.md) and
   carries no unaddressed advisory. Reverting to a signed, working, *vulnerable*
   binary is a real and easy mistake: the binary is legitimate, its signature
   verifies, and nothing in the deploy path objects. If the last known-good
   version is vulnerable, the correct action is to **fix forward under halt**,
   not to revert.
4. Redeploy per the local-build constraint — testnet images are built locally
   and pushed by SSH to the N5 host. **Never** pull from a registry during
   incident recovery; a compromised or merely stale registry image is exactly
   the thing you cannot afford here.
5. Verify convergence per §2 before resuming.

**Verification** — all nodes report the intended version; heights converge;
state roots match; `/readyz` clear on every node.

**Comms** — Operator channel at halt and at resume. If the upgrade was
externally announced, a public note stating the version reverted to and why.

---

## 5. Bridge / oracle / proving-system failure isolation

The principle for all three: **isolate the failing subsystem without halting the
chain**, if the chain itself is sound.

### Bridge

**Trigger** — exposure-cap breach, anomaly threshold, or challenge quorum.
Auto-halt triggers and thresholds are defined in
[`BRIDGE_SAFETY_RUNBOOK.md` §Auto-Halt Triggers](../BRIDGE_SAFETY_RUNBOOK.md).

**Authority** — Operator token (quorum 1) to halt. Resume requires the
prerequisites in that runbook's §"Resuming After Halt" — do not shortcut them.

**Steps / verification / comms** — as per BRIDGE_SAFETY_RUNBOOK. Not repeated
here.

### Oracle

**Trigger** — attestation quorum cannot be reached; an oracle is submitting
data that fails plausibility checks; an oracle key is suspected compromised.

**Authority** — Operator token **and** `WRFCOIN_ENABLE_ORACLE_ADMIN_API=1`.

**Steps**

1. **Do not mutate the oracle set on a live multi-node network.** It forks the
   state root (#1775). If the network has more than one node, the correct
   isolation is to halt the *oracle*, not to edit the on-chain set.
2. ⛔ **Key-epoch revocation is not executable on a live network** (dead-end
   table, row 1). `POST /transactions/signing-keys/revoke` exists but rejects
   any chain at height > 0. If an oracle key is compromised today, the
   available controls are: stop the oracle process itself, rotate
   `WRFCOIN_ORACLE_TOKEN` on the backend/collector so the compromised holder
   loses API access, and treat the key's future attestations as hostile until
   a revocation path lands. Record the compromise time for the post-mortem.
3. If quorum cannot be reached, the earn loop stalls. That is a degraded mode,
   not an emergency — it does not justify a chain halt.

**Verification** — `State.attestations` shows quorum resuming, or the oracle
is absent from `State.oracle_set` on **every** node with identical state roots.

**Comms** — Operator channel; notify affected station operators if rewards stall.

### Proving system (ZK)

**Trigger** — proofs failing verification, or proof generation exhausting node
memory.

**Authority** — Operator token.

**Steps**

1. ZK verification failures do **not** justify a chain halt on their own —
   verification failing closed is the system working. Establish first whether
   proofs are being *rejected* (safe) or *accepted wrongly* (an emergency).
2. Known live defect: proofs generated with the embedded
   `native-chain/zk-params/*.bin` bundles fail verification (#2240, open). If
   you are seeing this on a node that has not changed, it is that pre-existing
   defect, not a new attack.
3. Memory exhaustion during proving was the #1833 / #2038 vector (both now
   **closed** — keys are loaded from a per-config disk cache,
   `native-chain/src/zk_service.rs`). The residual risk is a *novel*
   caller-chosen circuit config triggering a fresh trusted setup (~2^64
   possible configs). Measured setup cost is ~70 MiB and under a second per
   call (`docs/zk/PROVING_COSTS.md`, on `main` since PR #2230 merged
   2026-07-22), so a single request cannot OOM a node — **concurrency** can.
   Rate-limit the endpoint; a memory cap alone will not fix it.

**Verification** — proof verification succeeds against a known-good fixture;
node RSS stable under load.

**Comms** — Operator channel.

---

## 6. Release-signing-key compromise

**Trigger** — repo secrets exposed, an unexpected signed release appears, or CI
shows a release job nobody authorized.

**Authority** — Repository administrator. This is the one procedure where the
operator token is irrelevant — the compromise is upstream of the chain.

**Steps**

1. **Revoke first, investigate second.** Disable the signing workflow and rotate
   the GitHub repo secrets immediately. Every minute the key is live is another
   minute an attacker can publish a trusted binary (THREAT_MODEL A5).
2. Enumerate every release signed since the earliest possible compromise. Treat
   all of them as suspect, not just the ones that look wrong.
3. Publish the list of affected version numbers and their true hashes.
4. Instruct every operator to verify their running binary against a
   independently-rebuilt hash — not against the signature, which is exactly what
   is no longer trustworthy.
5. Re-sign known-good releases under the new key; publish the new public key
   through a channel that does not depend on the compromised one.

**Verification** — old key revoked and rejected by the deploy path; every
operator confirms a matching rebuilt hash.

**Comms** — Public disclosure required, promptly, per
[`docs/SECURITY.md`](../SECURITY.md)'s responsible disclosure posture (note:
there is no repo-root `SECURITY.md`; the policy lives under `docs/`). A signing-key compromise affects everyone who trusts the
release channel, which is everyone.

---

## 7. Validator-key compromise

**Trigger** — a validator signs conflicting blocks or votes (equivocation), or
an operator reports key loss.

**Authority** — Operator token holder to isolate the node; governance path to
revoke the key epoch.

**Steps**

1. Isolate: set partition mode read-only on the affected node so it stops
   producing (§1's command).
2. ⛔ **Revocation dead end** (dead-end table, row 1): the epoch-revocation
   route refuses live chains, and no governance transaction applies
   `RevokeSigningKeyEpoch`. Until that path exists, isolation (step 1) plus
   token rotation (step 5) are the only implemented controls. When revocation
   does land, note its limit: it is epoch-based and forward-looking — it does
   not invalidate signatures the key has already produced.
3. Assess damage. Note the limit honestly: a `FinalityVote` binds no
   validator-set or epoch identity (#2256), so votes cast by the compromised key
   remain replayable under a later committee. Revocation alone does not
   neutralise votes already in flight.
4. If the key equivocated on finality, treat it as a §1 safety event — the
   finality invariant should already have halted the consumer.
5. Rotate the operator token on that host too. A host that leaked a validator
   key should not be assumed to have kept its bearer tokens.

**Verification** — the isolated node produces no blocks
(`GET /node/partition-status` shows read-only). Once a revocation path exists:
the revoked epoch recorded in `State.signing_keys` on every node with matching
state roots.

**Comms** — Operator channel; public note if the validator participated in
finalizing blocks that are now in question.

---

## 8. Failure modes already observed on this chain

Both of these have actually happened here. They are listed because an operator
who has read only generic incident material will misdiagnose them.

### 8a. Producer/verifier signing-domain split

**Observed** — 2026-07-08: the N5 collector (block producer) and the node
binary disagreed on the commit-auth signature domain version after a
consensus-side change landed; every produced block was silently rejected and
the chain halted for ~28 hours with no crash and no error burst — just no new
blocks. Resolved by wrfcoin/infra#638 (domain-version fallback in the
collector). The underlying exposure — blocks carry no producer signature at
all, so domain/version agreement is load-bearing — is #2290, folded into the
#2034 consolidated fresh-genesis cut.

**Trigger** — height stops advancing shortly after *either* side of the
producer/node pair is redeployed; `/readyz` may stay green; logs show
signature-verification rejections (or nothing at all) rather than errors.

**Diagnosis** — compare the build/commit of the producer and the node; check
whether a signing-domain or preimage change (chain-id binding, domain
registry) merged between them. A version skew here is this failure until
proven otherwise.

**Recovery** — redeploy producer and node from the same commit (local build,
§4 step 4's registry rule applies). No state is corrupted — production simply
resumes once the pair agrees.

### 8b. Block assembly silently dropping failed transactions

**Observed** — #2056 (open): the block-assembly path swallowed the `Err` from
transaction application — the API returned a `txHash` and the mempool accepted
the transfer, but it never reached a block; balances silently did not move.
Root-caused 2026-07-02; fix pattern in PR #2059.

**Trigger** — user reports "successful" transfers that never land; mempool
accepts, height advances, balances unchanged. No error anywhere, because the
error was eaten.

**Diagnosis** — pick one affected `txHash`: confirm the API accepted it, then
confirm it appears in **no** block body. Cross-check the sender's nonce and
balance on-chain versus what the API reported at submission time. If the tx
vanished between mempool and block, this is 8b, not a wallet bug.

**Recovery** — this is a code defect, not an operational state: nothing to
roll back (the transfers never applied). Halt is not required if the chain is
otherwise consistent. Escalate to engineering with the affected hashes; the
resubmitted transactions will apply once the swallowed-error path is fixed.

---

## Resuming after any halt

Resuming needs **more** authority than halting, deliberately.

1. Root cause identified and written down. "It seems fine now" is not a root
   cause.
2. A second person has reviewed the evidence independently.
3. Convergence verified per §2 — identical state roots, via
   `verify-two-node-convergence.sh`, not block hashes.
4. `/readyz` clear on every node with no `state_diverged` latch.
5. The invariant harness (`InvariantHarness::check_all`, #2100 — merged in
   PR #2260, 2026-07-22) reports no violations:
   `cargo test -p wrfcoin-shared-protocol --test invariant_harness` plus the
   registration-completeness check (`shipped_registration_is_complete`).
   Additionally confirm the finality consumer logged no `INV-FINALITY-1`
   violation (`shared-protocol/src/invariants/finality.rs`).
6. Only then clear partition mode, one node at a time, verifying convergence
   between each.

---

## Communications

| Audience | When | Channel |
|---|---|---|
| Operators | Immediately on any halt | Operator channel |
| Station operators / contributors | When the earn loop is affected > 30 min | Status page |
| Public | Within 30 min for an externally-attached testnet halt; promptly and unconditionally for a signing-key compromise | Status page, repo |
| Security reporters | Per [`docs/SECURITY.md`](../SECURITY.md) | security@ mailbox |

Say what is known, what is not yet known, and when the next update will come.
On testnet, always state that balances are valueless and no funds are at risk —
it is true, and it prevents the wrong kind of alarm.

---

## Open gaps

Recorded here rather than left implicit, because a runbook that hides its own
weak points is a liability.

1. **No multi-signature anywhere.** Every row of the authority matrix that says
   quorum 1 is a single point of failure. The "second reviewer" requirements in
   §3 and §Resuming are process, not enforcement — nothing stops one token
   holder acting alone. Mainnet needs threshold authorization.
2. **No enforced break-glass procedure.** There is no separate emergency
   credential; the everyday operator token is also the emergency one, so
   emergency use is indistinguishable from routine use in the audit trail.
3. **Audit trail is not tamper-evident.** `audit_events` lives in the same state
   an operator with the same token can mutate.
4. **`#2240` is live**: the embedded ZK key bundles produce proofs that fail
   verification. §5's ZK procedure works around it rather than fixing it.
5. **`#2256` is live**: finality votes are not bound to a validator set, which
   limits what §7's revocation could achieve even once a revocation path
   exists.
6. **The dead-end table at the top is part of this document's contract.** The
   two entries with no tracking issue (live key revocation; chain
   rollback/restore tooling) were reported to the sprint manager for filing on
   2026-07-22; update the table when the issues exist.
