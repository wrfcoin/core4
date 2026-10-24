# Agents as earning participants — Phase B scope

Status: **Accepted (owner, 2026-10-24).** A scoping document: it lists candidate slices, their
sizes, dependencies and risks. It authorizes no implementation, consensus change,
payout rule or deployment; the owner's answers are in "Decision" at the end.

Date: 2026-10-23.

Refs: `wrfcoin/core4-archive#3604`, a private epic, "LLM agents and the forecaster / analyst
earning loop". Its principle is that **an agent is a wallet**: the protocol pays for work it can
check without trusting the worker, and does not care whether a human, a script or an LLM did it.
Hence the order: forecasts first (self-checking against later observations), services second
(receipts already exist), reports third (needs a reproducibility primitive that does not exist
yet).

Testnet tokens have no monetary value. Nothing here promises payouts.

## 1. What Phase A delivered

Phase A's goal was the forecaster role, live on testnet. These pieces merged to the public
repositories on 2026-10-23:

| Epic item | Delivered | Where |
|---|---|---|
| A3 Forecaster kit | Kit that prepares commit and reveal files; `wrfcoin forecast commit/reveal --validate-only` | `wrfcoin/core4#21` (`tools/forecast-kit/`) |
| A4 Accuracy track record | Public forecaster track record from the chain-rewards index | `wrfcoin/backend#17` (API), `wrfcoin/frontend#7` (`/forecasters` pages) |
| A5 Quotas for agent traffic | Bounded wallet forecast request quotas | `wrfcoin/core4#16`; design `docs/adr/2026-10-19-forecast-cell-lead-quotas.md`, `docs/forecast/http-quotas.md` |
| Scoring fidelity | Score only the variables a station reports | `wrfcoin/core4#17` (`docs/forecast/variable-subset-scoring.md`) |
| Model quality | A learned model trained on real station observations, with held-out metrics computed by the chain's `forecast-score` crate | `wrfcoin/core4#20` (`docs/forecast/scorer-truth-source.md`) |

Still open from Phase A. Phase B depends on these:

| Epic item | State on public main | Notes |
|---|---|---|
| A1 Receipt signer | **The command exists**: `wrfcoin forecast sign-receipt` (`tools/cli/WrfcoinCLI/src/commands/forecast.rs:45-70`). It signs with an oracle-set key; production uses the oracle HSM role. | **Operating** it on the live testnet is not established by this document: who runs it, for which reveals, and against which truth source (`docs/forecast/scorer-truth-source.md` lists that as undecided). Without a receipt, a reveal is scored at the baseline. |
| A2 Forecaster key binding | **Design only**: `docs/adr/2026-10-19-3290-forecast-identity-binding.md` (Proposed, post-freeze, consensus) | Commits gossiped between nodes skip the HTTP route's wallet check until this lands. |
| A6 Anti-gaming scoring | **Not active.** The port into `forecast-score` behind a feature is still open (`wrfcoin/core4-archive#3244`). | Climatology dumping and copied forecasts are only partly defended. |

**The Phase A exit criterion is not yet shown:** one external wallet, run by someone other than the
owner, earning a forecast credit above baseline, visible in wallet history and on the leaderboard.
It needs A1 in operation.

## 2. Phase B candidate slices

Sizes: **S** ≤ 1 day, **M** 2–4 days, **L** ≥ 1 week. Sizes are for the first useful slice, not the
whole idea.

### B-0. Close Phase A's payout path (prerequisite)

| | |
|---|---|
| What | Run the receipt signer for testnet reveals with a ratified truth source, and record the realised count of receipt-scored versus baseline-scored reveals. |
| Size | **M** (operations and documentation; the code exists) |
| Depends on | Owner choice of truth source (`docs/forecast/scorer-truth-source.md`); oracle key custody |
| Risks | A signer that scores against the wrong truth pays for the wrong thing. A signer outage silently reverts everyone to baseline. |
| Ratify first | Truth source; who operates the signer; whether A2 key binding must activate first, since without it a credit can be claimed for someone else's forecast through gossip |

### B-1. Forecaster key binding (consensus)

| | |
|---|---|
| What | Implement `docs/adr/2026-10-19-3290-forecast-identity-binding.md`: the forecaster address must equal the address derived from the authorization key, using a new versioned authorization domain behind an unscheduled `ConsensusFeature`. |
| Size | **L** |
| Depends on | ADR ratification; the freeze being lifted; gate-off parity and replay differential |
| Risks | Wire and authorization change. An activation mistake splits the chain. |
| Ratify first | The ADR itself, and the activation-height policy |

### B-2. Reproducible-job primitive (design, then build)

| | |
|---|---|
| What | One **work unit** for any paid computation. It names its inputs (a data range, station set and variables, pinned by digest), the job (container image digest, arguments, seed) and its output (digest plus a summary). Verification is a second-party rerun that must match within a **declared tolerance**. GPUs are never required to be bitwise-identical. A signed attestation pays the worker and the verifier. Reuse the tolerance-based spot-recomputation model being designed for distributed forecast compute (`wrfcoin/core4-archive#3557`), so forecasts, ensembles and reports share one verification model. |
| Size | **M** (ADR); **L** (first implementation) |
| Depends on | #3557 alignment; a verifier role (who reruns, how chosen, how paid) |
| Risks | Verifier collusion, so random assignment with spot checks of verifiers is needed. Tolerance set too loose or too tight. Storage of inputs and outputs. |
| Ratify first | Whether reports and ensembles share one work-unit format; how verifiers are chosen and paid; dispute and slashing rules |

### B-3. Collaborative / ensemble forecasts (off-chain first)

| | |
|---|---|
| What | Several independent agents or teams forecast the same station and lead. An **open, deterministic combiner** (equal-weight mean, median, or weights from each member's *trailing* track record) produces a group forecast. **Each member commits its own forecast from its own wallet.** The group forecast may be committed by a designated group wallet as one more forecast. Everything is scored by the same `forecast-score` crate, so "does the ensemble beat its members?" is measured, not asserted. |
| Rules that keep it checkable | Member and group files pin method id, model digest, input-manifest hash, software versions and seed. The group file records every member digest and the exact weights, and its digest is fixed **at commit**, before the valid time, so weights cannot be chosen after the outcome. Forecasts committed after another member's reveal for the same cell and lead are ineligible. The combiner is reproducible byte-for-byte from public data. |
| Credit | The chain pays each wallet for its own scored forecast, and the ensemble earns separately as its own forecast. **Any split inside a group is an off-chain convention**, paid by ordinary transfers and checkable against a published rule. One proposed rule is leave-one-out marginal skill: `weight_i = max(0, skill(group) − skill(group without i))`. No consensus change. |
| Size | **S** (file formats and combiner with fixtures); **M** (a two-team validate-only dry run, then a live run) |
| Depends on | B-0 (receipts), so members are scored above baseline; A6 anti-gaming, so one model submitted from many wallets is caught; B-1 for gossip-safe identity |
| Risks | Many wallets submitting one model (needs quotas, key binding and copy detection). Collusion inflating marginal credit (only chain-scored outcomes count; correlated members get diminishing weight). Ensembles crowding out individual forecasters' rewards (Q4). |
| Ratify first | Whether a group wallet may commit the ensemble as a forecast; the attribution rule (leave-one-out or alternatives); the late-commit eligibility rule |

### B-4. Track record → reward linkage

| | |
|---|---|
| What | Decide whether, and how, a forecaster's **trailing** skill affects anything beyond each forecast's own receipt. |
| Options | (a) **None**: pay per receipt only, as today. Simplest and least gameable. (b) **Admission only**: a stronger track record earns larger quotas or priority, with no extra money. (c) **Bounties**: a governance-funded bounty for a model that beats the skill baseline on a held-out window, scored by the same receipts (the white paper's developer rewards). (d) **Multiplier**: trailing skill multiplies future rewards. This is a consensus change, and it is gameable through early luck, cherry-picked cells or Sybil restarts. |
| Size | (a) none; (b) **S**; (c) **S** design + **M** build; (d) **L** (consensus) |
| Depends on | A stable, public track-record definition (A4 computes one off-chain today). (c) needs a governance treasury. (d) needs B-1 and A6. |
| Risks | Reputation feedback loops lock in early winners. A wallet reset can launder a bad record unless identity is bound. |
| Ratify first | Which of (a)–(d); for (c), the funding source, which is tied to the tokenomics ADR (draft, `wrfcoin/core4#27`) |

### B-5. First paid report type (after B-2)

| | |
|---|---|
| What | A daily regional **observation-quality report** (coverage, outliers, station drift), chosen because its inputs are already on chain. It is paid only through the B-2 work unit. Air quality, soil moisture and water quality follow when those data streams exist. |
| Size | **M** |
| Depends on | B-2 built; a verifier pool |
| Risks | Reports that nobody uses (ask who consumes them); privacy of station-level drift findings |
| Ratify first | That reports are paid at all before verifiers exist (default: no); the first report type |

### B-6. Storage keepers (lower priority for this phase)

| | |
|---|---|
| What | Extend the bonded snapshot service to archive retrieval with random audits (the white paper's "data keepers"). A failed audit forfeits the epoch share. |
| Size | **L** (core4 plus the storage-providers crates) |
| Depends on | Node-share settlement in operation; audit challenge design |
| Ratify first | Whether it belongs in this phase or with the storage track |

## 3. Suggested dependency order

For orientation only; not a decision.

```
B-0 receipts in operation ──┬──> B-3 ensembles (off-chain) ──> B-4 linkage (a/b/c)
B-1 key binding (consensus) ┘                                   │
A6 anti-gaming ─────────────────────────────────────────────────┘
B-2 work-unit ADR ──> B-2 build ──> B-5 first report
B-6 storage keepers: independent track
```

## 4. Explicitly out of scope for Phase B

- **On-chain ensemble transactions or on-chain reward splits.** Group credit stays an off-chain,
  reproducible convention until it shows demand. Adding it would be separate consensus work.
- A separate "agent" identity class or registry. An agent is a wallet.
- A paid or metered API. The owner is undecided; quotas are protection only.
- Paying for written reports before the reproducible-job primitive and verifiers exist.
- Phase C GPU compute on contributor hardware (`wrfcoin/core4-archive#3553`). It waits on its own
  measurement and licence stories.
- Any change to emission shares or supply. That belongs to the tokenomics ADR.
- Value, price or return language in any user-facing copy.

## 5. Questions for the owner

1. **Payout path first:** who operates the receipt signer on the testnet, against which truth
   source, and must forecaster key binding (B-1) activate before receipts are issued?
2. **Ensembles:** may a group wallet commit an ensemble as its own forecast? If so, which
   attribution rule inside a group (leave-one-out marginal skill, equal split or another)?
3. **Late commits:** adopt the rule that a forecast committed after another member's reveal for the
   same cell and lead is ineligible?
4. **Reward pool:** do ensembles and individual forecasts draw from the same contribution slice, or
   does a cap per cell and lead stop ensembles crowding out individuals?
5. **Track-record linkage:** (a) none, (b) admission only, (c) bounties, or (d) multiplier?
6. **Reports:** confirm that no written report is paid before the work-unit primitive and a verifier
   pool exist, and that the first report type is the observation-quality report.
7. **Storage keepers:** in this phase or a separate storage track?

## Decision (accepted 2026-10-24)

Owner ruling, 2026-10-24: "Merge as accepted, recommended answers." Each open question gets the
answer this document itself recommends, quoted with its line. Where the document makes no
recommendation, the question was left open for the owner and is answered below as `Decision (owner, 2026-10-24)`: none was invented.

Accepting this scope authorizes no implementation, payout rule or deployment by itself; each slice
still lands through its own PR. Answers below that the document does not recommend are the owner's, recorded as `Decision (owner, 2026-10-24)`.

1. *Payout path: who operates the receipt signer, against which truth source, and must key binding
   (B-1) activate before receipts?* **Decision (owner, 2026-10-24): Oracle operators sign receipts. The forecaster key binding (#3290, B-1) must land first, before receipts activate.** Relevant text (not a recommendation):
   line 56: "| Ratify first | Truth source; who operates the signer;". Listed as items to ratify; no
   recommendation.
2. *Ensembles: may a group wallet commit an ensemble as its own forecast, and which attribution rule
   inside a group?* **Yes, a designated group wallet may commit the ensemble as one more forecast.
   Attribution: leave-one-out marginal skill, as an off-chain convention paid by ordinary transfers
   (no consensus change).** Recommended by the document: line 82: "The group forecast may be
   committed by a designated group wallet as one more forecast."; line 84: "One proposed rule is
   leave-one-out marginal skill"; line 84: "**Any split inside a group is an off-chain
   convention**".
3. *Late commits: is a forecast committed after another member's reveal for the same cell and lead
   ineligible?* **Adopted: such a forecast is ineligible.** Recommended by the document: line 83:
   "Forecasts committed after another member's reveal for the same cell and lead are ineligible.".
4. *Reward pool: same contribution slice for ensembles and individuals, or a cap per cell and lead?*
   **Decision (owner, 2026-10-24): Cap per cell and lead time (not the same contribution slice for ensembles and individuals). Implementation requires a separate consensus PR + owner deploy.**
5. *Track-record linkage: (a) none, (b) admission only, (c) bounties or (d) multiplier?*
   **Decision (owner, 2026-10-24): (c) Bounties.** Relevant text (not a recommendation): line 95: "Simplest and least
   gameable". Stated as a property of option (a), not a recommendation.
6. *Reports: no written report paid before the work-unit primitive and a verifier pool exist; first
   type is the observation-quality report?* **Confirmed: no written report is paid before the
   work-unit primitive and a verifier pool exist, and the first report type is the daily regional
   observation-quality report.** Recommended by the document: line 109: "That reports are paid at
   all before verifiers exist (default: no)"; line 105: "A daily regional **observation-quality
   report**"; line 138: "Paying for written reports before the reproducible-job primitive and
   verifiers exist.".
7. *Storage keepers: in this phase or a separate storage track?* **Decision (owner, 2026-10-24): In this phase (B-6 is included in Phase B, not a separate storage track).** Relevant
   text (not a recommendation): line 111: "### B-6. Storage keepers (lower priority for this
   phase)"; line 129: "B-6 storage keepers: independent track". The heading and the orientation-only
   graph point in different directions.
