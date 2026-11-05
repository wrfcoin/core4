# ADR: the report work unit — one reproducible, re-runnable job format for paid analysis

Status: **PROPOSED (draft for owner ratification).** Design only. It authorizes no payout, consensus
change, transaction type, schedule entry or deployment. The owner's open questions are listed at
the end, each with a recommended answer.

Date: 2026-10-28.

Refs: `wrfcoin/core4-archive#3604` (epic, story B2), `wrfcoin/core4-archive#3557` (distributed
forecast work unit and verification), `docs/design/agents-earning-phase-b-scope.md` §B-2 and §B-5
(accepted 2026-10-24). Code references are to public `core4` main `cc7f4d4`.

## Context

Phase B pays for work the protocol can **check without trusting the worker**. An agent is a wallet;
the protocol never asks whether a human, a script or an LLM did the work.

- **Forecasts** check themselves: a later observation scores them, and an oracle-signed
  `ForecastVerificationReceipt` carries the score
  (`forecast-score/src/blockchain.rs:296-347`; consensus mints at `receipt.metrics.skill_score`,
  `shared-protocol/src/consensus/proof_of_publishing.rs:5378-5393`).
- **Services** are checked by a second party's signed receipt: a consumer signs a
  `NodeServiceReceipt` for a snapshot it fetched, verified and applied
  (`shared-protocol/src/consensus/node_service_receipt.rs:42-58`), and a bonded claim pairs it
  with the server's signature (`BondedServiceClaim`, `node_share_contracts.rs:213-219`).
- **Reports** (an observation-quality summary, a drift analysis, an ensemble statistic) have no
  later observation to check them and no consumer whose signature proves correctness. The only
  check that does not trust the analyst is **a second party running the same job on the same
  inputs and getting the same answer within a declared tolerance.** No primitive for that exists.

#3557 designs the same primitive for distributed forecast compute: one ensemble member per
contributor GPU, verified by **tolerance-based spot recomputation**, with no promise of bitwise
determinism across GPUs or drivers. Reports should use #3557's structure so forecasts, ensembles
and reports share **one** verification model, one receipt shape and one dispute path. The scope
document already requires that (B-2, "Reuse the tolerance-based spot-recomputation model").

## Decision drivers

1. **Checkable without trust.** Every paid number is reproducible by someone other than the worker
   from public, digest-pinned inputs.
2. **No bitwise GPU determinism.** Agreement is within a declared tolerance. A CPU-deterministic
   job may declare tolerance zero; nothing else is required to.
3. **One model for forecasts, ensembles and reports** (#3557 alignment).
4. **Reuse existing signed shapes.** Domain-separated preimages (`DomainSeparated`,
   `BodyBuilder`), the r20.9 bonded-receipt pattern, and the identity binding of #3290.
5. **No payment before verifiers exist** (owner, 2026-10-24, Phase B Q6).

## 1. The work unit

A work unit has three parts. Each part is a canonical document whose digest is a SHA3-256 hash over
a domain-separated, length-prefixed body (the `BodyBuilder` encoding already used by
`NodeServiceReceipt`). JSON is a transport only; the digest never covers JSON bytes.

### 1.1 Input manifest — *what the job reads*

| Field | Meaning |
|---|---|
| `schema_version` | `1` |
| `chain_id`, `network_id` | Chain identity, so a unit cannot be replayed on another chain |
| `range` | Inclusive block-height range `[from_height, to_height]` the inputs are drawn from. A wall-clock range is **not** allowed: heights are what every node agrees on |
| `stations` | Sorted, de-duplicated `station_id` list, or the literal `*` for "every station registered at `to_height`" |
| `variables` | Sorted canonical variable names (`temperature_c`, `pressure_hpa`, …) |
| `sources` | Each non-chain input (e.g. an `oracle-reference` analysis field) as `{name, uri, sha3_256}`. Empty for chain-only jobs |
| `inputs_digest` | SHA3-256 over the canonical extraction of the chain inputs (§1.4), so a verifier proves it read the same bytes before it compares any output |

### 1.2 Job — *what runs*

| Field | Meaning |
|---|---|
| `job_type` | Stable name, e.g. `obs_quality_daily/v1` (the first type, B3) |
| `runner` | Either `{kind: "container", image_digest: "sha256:…"}` (an OCI digest, never a tag) or `{kind: "builtin", name, version}` for a job compiled into the `wrfcoin` CLI |
| `args` | Ordered list of strings |
| `seed` | `u64`. Every source of randomness is derived from it. A job that uses none still records `0` |
| `tolerance` | The declared agreement rule (§2.2) |
| `resource_hint` | Advisory CPU/GPU/memory/seconds, for verifier scheduling only. Never part of agreement |

### 1.3 Output — *what it produced*

| Field | Meaning |
|---|---|
| `output_digest` | SHA3-256 over the canonical output body. For a tolerance-zero job this is the agreement test |
| `summary` | A small, typed vector of the numbers the tolerance applies to (e.g. per-station coverage fraction, outlier count, drift estimate). Bounded length (proposed ≤ 4 096 numbers) so a verifier compares numbers, not blobs |
| `artifact` | Optional `{uri, sha3_256, bytes}` for the full output (a CSV, a map tile). Stored off-chain (§5) |

The **work-unit id** is `H(domain "wrfcoin.work_unit.v1" ‖ input_manifest_digest ‖ job_digest)`. It
is fixed before any output exists, so a worker cannot pick inputs after seeing the answer.

### 1.4 Canonical chain-input extraction

A job that reads chain data reads it through one documented extraction: the accepted
`ContributionReveal` payloads in `range` for `stations`, projected onto `variables`, ordered by
`(height, tx_index)`. Floats are encoded as their IEEE-754 bit patterns and timestamps as
integers. Two honest nodes at the same tip produce identical bytes, so `inputs_digest` is checkable
by anyone with a synced node or an archive (§5).

## 2. Verification — #3557's spot-recomputation structure, applied to reports

### 2.1 Who verifies, and how often

- **Assignment is random and late.** Verifiers for a unit are drawn by a deterministic lottery seeded
  by the hash of the first block **after** the worker's commitment (§3), over the bonded verifier
  pool, excluding the worker's own wallet and any wallet sharing its bond. Neither the worker nor a
  verifier can choose the pairing.
- **Sampling rate.** Every report unit is re-run by at least **one** verifier while the pool is
  small. #3557 proposes sampling for high-volume ensemble members; reports are low-volume and
  have no later observation, so this ADR proposes 100 % recomputation for reports (Q2).
- **Verifiers are themselves spot-checked.** A fraction of assignments (proposed 10 %) is a
  **known-answer unit**: a unit already settled by two independent verifiers. A verifier that
  disagrees with a known answer is treated as a failed verification (§4). The "spot checks of
  verifiers" idea comes from the white paper, carried into a comment on #3557 (not #3557's own text),
  and is applied here against collusion.

### 2.2 The tolerance rule

The job declares, before running, one of:

| Rule | Agreement when |
|---|---|
| `exact` | `output_digest` is equal. Default for CPU-deterministic built-in jobs |
| `abs(ε)` | every `summary[i]` agrees within `ε` |
| `rel(ε)` | every `summary[i]` agrees within `ε · max(|a|, |b|, floor)` |
| `rmse(ε)` | RMSE over the `summary` vectors ≤ `ε` (the #3557 metric for ensemble fields) |
| `crps(ε)` | CRPS difference ≤ `ε`, for probabilistic summaries (#3557) |

Tolerance is part of the job digest, so it cannot be loosened after the fact. Per-`job_type` ceilings
are governance parameters: a job type whose declared ε exceeds its ceiling is not eligible for
payment. This stops a worker from declaring a tolerance so loose that anything matches.

**Never bitwise GPU determinism.** A container job on a GPU declares `rmse` or `rel`. Agreement
is a property of the declared tolerance, not of identical floating-point bytes.

### 2.3 The attestation receipt

A verifier that re-runs a unit signs a `WorkUnitAttestation`, a domain-separated preimage in the
style of `NodeServiceReceipt`:

```
domain  "wrfcoin.work_unit_attestation.v1"
fields  chain identity, work_unit_id, worker (20-byte address), verifier (20-byte address),
        worker_output_digest, verifier_output_digest, verdict (agree | disagree | inputs_mismatch),
        max_deviation (fixed-point), verified_at_height, nonce
```

The worker's own submission carries the output digest and summary and is signed by the worker's
key. The **#3290 binding applies to both roles**: the worker and verifier addresses must equal the
addresses of the keys that signed, so a third party cannot claim someone else's work or
verification. The forecast-lane binding that would set the precedent is pending in core4#54 (not on
main yet).

A unit is **settled** when its required attestations agree. Payment (§4) is made from settled units
only.

## 3. Lifecycle

1. **Commit.** The worker publishes `work_unit_id` plus `H(output ‖ salt)`. Nothing about the
   answer is visible yet. This mirrors the forecast commit/reveal lane.
2. **Assign.** The block after the commit seeds verifier selection (§2.1).
3. **Reveal.** The worker reveals the output and salt within a window (proposed 512 blocks).
4. **Re-run.** Each assigned verifier recomputes from the input manifest, independently, and
   commits its own output hash before it sees the worker's reveal. The verifier's commit can
   precede the worker's reveal, so a verifier cannot copy the answer.
5. **Attest.** Verifiers reveal and sign attestations.
6. **Settle or dispute** (§4).

## 4. Payment, dispute and slashing

*Not activated by this ADR.* No report is paid before this primitive and a verifier pool exist
(owner, 2026-10-24).

- **Paid parties.** A settled unit pays the worker and each verifier from the same per-job-type
  budget. The split is a governance parameter (Q3). The budget source is the tokenomics ADR's
  business, not this one: this ADR changes no emission slice (`wrfcoin/core4#27` is unratified).
- **Bonds.** Workers and verifiers post a bond (the r20.9 bonded-service model,
  `NODE_BOND_WRF = 250`, `node_share_contracts.rs:20`, is the precedent).
- **Agree.** The worker and verifiers are paid. Nothing is slashed.
- **Disagree.** One additional verifier is drawn (a tie-break). The majority within tolerance is
  correct. The minority party forfeits its share of that unit, and forfeits a slice of bond after
  repeated failures (proposed: three in a rolling 1 024-block window, `NODE_EPOCH_BLOCKS`). A
  **single** disagreement never slashes bond: GPU nondeterminism near the tolerance edge is
  expected, and the tolerance ceiling, not the slash, is what bounds it.
- **`inputs_mismatch`.** The verifier could not reproduce `inputs_digest`. The unit is void, and
  nobody is paid or slashed. The worker resubmits against a confirmed input set. This keeps
  "my node was behind" apart from fraud.
- **Known-answer failure** (§2.1). The verifier forfeits that assignment's share. Repeated failures
  slash on the same schedule as a minority worker.
- **No reporter reward.** Consistent with the operator-only fraud route ruling, a dispute is raised
  by the protocol's own tie-break, not by a bounty-seeking third party.

## 5. Inputs and outputs storage

- Chain-only inputs need no storage beyond an archive of blocks: `inputs_digest` is recomputable.
  Retrieval of old ranges is the storage-keeper story (B1 / `wrfcoin/core4-archive#3564`), which
  makes "a verifier can still fetch the range" an audited property instead of a hope.
- `artifact` files are content-addressed (`sha3_256`) and may live on IPFS, Arweave or a keeper
  archive. The digest, not the location, is what is attested.
- Station-level drift findings can be sensitive (Phase B §B-5 risk). The first report type publishes
  regional aggregates. Per-station findings are a separate, owner-approved step.

## 6. How forecasts and ensembles map onto the same model

| | Forecast (today) | Ensemble member (#3557) | Report (this ADR) |
|---|---|---|---|
| Inputs pinned by | `ForecastArtifactManifest.input_hash` | init time, member seed, perturbation | input manifest §1.1 |
| Output pinned by | `output_hash`, committed before valid time | field digest | `output_digest` |
| Checked by | a later observation, via an oracle receipt | spot recomputation within `rmse/crps(ε)` | recomputation within declared tolerance |
| Receipt | `ForecastVerificationReceipt` | `WorkUnitAttestation` | `WorkUnitAttestation` |
| Identity | #3290 key binding | #3290 | #3290 |

One attestation type and one dispute path serve both #3557 and reports. A forecast keeps its
observation-scored receipt, because a later observation is a stronger check than a rerun.

## 7. What this ADR deliberately does not do

- No transaction type, `ConsensusFeature`, state field or emission change. A follow-up
  implementation ADR names the transactions (commit, reveal, attest) and their gate, behind an
  unscheduled feature, with gate-off parity.
- No payment for written prose. A report is paid for its **reproducible numbers**; narrative text
  around them is unpaid.
- No zkVM requirement. A zk proof of execution (#2182) is a later alternative for job types small
  enough to prove. Recomputation is the default because it works for GPU jobs today.

## Open questions for the owner (each with a recommended answer)

1. **One format for reports and ensembles?** *Recommended: yes.* Reports and #3557 ensemble
   members share `WorkUnitAttestation` and the dispute path. Only the tolerance rule differs.
2. **Sampling rate for reports.** *Recommended: 100 % recomputation by one randomly assigned
   verifier while the pool has fewer than 20 bonded verifiers. Revisit at 20.* Reports are
   low-volume and have no later observation to catch errors.
3. **Worker / verifier split.** *Recommended: 70 / 30 of the unit's budget, verifier share split
   equally among assigned verifiers.* Verification must pay enough that verifiers actually re-run
   jobs instead of rubber-stamping, and known-answer units catch the rubber stamp.
4. **Who may verify?** *Recommended: any bonded wallet that is not the worker and does not share
   the worker's bond, chosen by the post-commit lottery.* No allow-list. An agent is a wallet.
5. **Slashing threshold.** *Recommended: forfeit the unit share on any minority verdict; slash bond
   only after three minority verdicts in 1 024 blocks.* One disagreement near a GPU tolerance edge
   must not cost a bond.
6. **Tolerance ceilings.** *Recommended: `exact` for built-in CPU jobs (including the first report
   type); per-job-type ceilings set by governance for container jobs before any is eligible.*
7. **Is the first report type the daily regional observation-quality report?** *Recommended: yes*
   (already confirmed in Phase B Q6). Built as a built-in, tolerance-`exact` job (story B3), with
   no payment path until this ADR is ratified and a verifier pool exists.
