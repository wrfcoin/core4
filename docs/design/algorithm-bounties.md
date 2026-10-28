# Design: algorithm bounties — governance-funded, scored by the same receipts

Status: **DRAFT for owner ratification.** Design only. It authorizes no payout, emission role,
transaction type, consensus change or treasury spend. Open questions are at the end, each with a
recommended answer.

Date: 2026-10-28.

Refs: `wrfcoin/core4-archive#3604` (epic, story B4). `docs/design/agents-earning-phase-b-scope.md` §B-4:
owner, 2026-10-24, "(c) Bounties", meaning track record pays through bounties, not through a reward
multiplier. Tokenomics ADR `docs/adr/2026-10-23-3561-tokenomics-reconciliation.md`, accepted: the owner
chose **O3-c**, emission-funded pools with no premine (line 166: "a governed 'development' … role paid
per block … It needs a new `EmissionRole` and governance over spending"). Its economics are not
ratified. Code references are to public `core4` main `cc7f4d4`.

Testnet tokens carry no promised value. A bounty is a test credit for a reproducible result, not a
prize of money.

## The principle

A bounty pays for a **model that beats the skill baseline on a held-out window, scored by the same
receipts that pay individual forecasts.** Nothing is judged by a person reading a model card. The
evidence is a set of oracle-signed `ForecastVerificationReceipt`s that anyone can verify, aggregated by
a published rule. An agent is a wallet: the bounty does not care who or what produced the forecasts.

## 1. Where the money would come from: the governance treasury today (finding)

The epic asks to reuse the existing governance treasury routes. They exist, but **nothing behind them
moves funds on main**:

| Surface | Where | What it actually does |
|---|---|---|
| `GET /governance/treasury`, `GET /governance/treasury/transactions` | `native-chain/src/bin/launch_api/routes/mod.rs:116-117`, `routes/governance.rs:1255` | Reads `governance.treasury_balance` and `treasury_transactions` |
| `POST /governance/treasury/deposit`, `POST /governance/treasury/withdraw` | `routes/mod.rs:118-119`, `routes/governance.rs:1353`, `:1392` (Operator role) | Enqueues `TransactionType::TreasuryDeposit` / `TreasuryWithdraw` |
| Applying those transactions | `shared-protocol/src/blockchain/state.rs:8021-8022` | **Explicit no-op** in `apply_protocol_transaction`'s inventory |
| The balance itself | `shared-protocol/src/invariants/balance.rs:201` | "`governance.treasury_balance` is declared but never mutated" |
| `ProposalType::FundingProposal { recipient, amount, purpose }` | `shared-protocol/src/governance/proposal.rs:36`, executed at `governance/implementation.rs:156` | **Logs only**: "Funding proposals would actually transfer tokens in a real implementation" |
| Submitting any governance proposal on chain | `shared-protocol/src/mempool/pool.rs:450`, `consensus/proof_of_publishing.rs:8513` | **Disabled** pending from-bound proposer authorization (`wrfcoin/core4-archive#2024`) |

So a bounty cannot be funded or paid through governance today. The design therefore has two phases.

## 2. Phase 1 (no consensus change): a published bounty, an off-chain evaluator, a manual transfer

1. **Announce.** The owner (or later governance) publishes a **bounty spec** (§3) with its digest, before
   the held-out window opens, together with an **escrow wallet** funded by ordinary transfers. The
   wallet's balance is public on chain. That is the honest stand-in for a treasury that does not exist
   yet.
2. **Compete.** Competitors forecast through the normal lane, commit → reveal → receipt, from the wallet
   they enter with. Nothing about the bounty changes the forecast lane.
3. **Evaluate.** After the window closes and the last receipts are signed, a deterministic evaluator
   (a `forecast-score` crate function plus a kit script, like the A1 scorer job) reads the window's
   receipts and computes every entrant's result (§4). Anyone can rerun it from the same receipts and
   get the same ranking.
4. **Pay.** The escrow pays the winner(s) by ordinary transfer. The transfer memo names the bounty
   digest and the evaluator output digest. A dispute window (proposed 7 days) precedes payment, and
   anyone who reruns the evaluator and gets a different result can say so publicly.

## 3. The bounty spec (digest-pinned before the window opens)

| Field | Meaning |
|---|---|
| `bounty_id`, `version` | Stable identity |
| `cells` | The scored cells: station ids today; a canonical geo cell once one is ratified |
| `leads` | Lead windows, e.g. `[1]` for 1–6 h |
| `window` | `valid_at` range `[start, end]`, entirely **after** the spec is published |
| `skill_definition` | The receipt definition in force (`rmse_sigma` or `mae_clim`; see core4#55), and whether anti-gaming is on (core4#56) |
| `baseline` | The reference the model must beat, scored by the **same** scorer on the same cells and times. Proposed: the better of persistence and climatology, which is exactly what anti-gaming already measures against, plus a named reference model (e.g. the kit's `persistence-trend/v1`, or the trained `learned-conv1d/v1` from core4#20) entered by the bounty's author |
| `margin` | How much better than the baseline counts as beating it (e.g. +0.05 mean skill) |
| `min_coverage` | The fraction of (cell, lead, issue time) slots an entrant must forecast (e.g. 90 %). **Missing slots score 0** |
| `amount`, `split` | Test-credit amount; split rule (winner-takes-all, or pro rata above the margin) |
| `escrow` | The escrow address |

## 4. Scoring: the same receipts, a fixed grid, no cherry-picking

For each entrant wallet `w`:

```
slots(w)  = every (cell, lead window, issue hour) in the spec's grid inside the window
skill(s)  = the receipt skill_score of w's revealed forecast for slot s, verified:
              receipt signature valid under the chain binding, signer in the oracle set,
              manifest digest and commit id bound (the same checks consensus applies);
            0 if w has no valid receipt for s (missing, unreceipted, or below the baseline
              and therefore never receipted by the scorer job)
score(w)  = mean over slots(w) of skill(s)
eligible  = coverage(w) >= min_coverage and score(w) >= baseline_score + margin
```

Scoring the **whole grid**, with missing slots as zero, is what stops cherry-picking only the easy
hours. A fixed window that opens after the spec is published stops tuning to a known period. Because
the bounty reads the same receipts that pay daily forecasts, a model cannot be scored differently for
the bounty than for its daily credit.

## 5. Gaming, and what stops it

| Attack | Defence |
|---|---|
| One model entered from many wallets | Entries are declared up front with the model digest (`manifest.model_id` and output-hash lineage). One payout per model digest; duplicates split one share. |
| Copying climatology, persistence or NOAA | Anti-gaming (core4#56) zeroes copied variables in the receipts themselves. |
| Copying another entrant | The late-commit rule (Phase B Q3, adopted): a forecast committed after another entrant's reveal for the same cell and lead is ineligible. |
| Early luck | A minimum window length (proposed 14 days) and minimum slot count. Reported with a bootstrap confidence interval; pay only if the lower bound beats the baseline. |
| Identity theft of someone's forecasts | The forecaster key binding (core4#54) once active. Until then, entries count only forecasts submitted through the bearer-bound route (`wrfcoin/core4-archive#3114`). |
| A lying evaluator | The evaluator is deterministic and public. Every input is a signed receipt. Anyone can rerun it during the dispute window. |

## 6. Phase 2 (consensus, each a separate gated slice, after the tokenomics economics are ratified)

1. **A real treasury.** Make `TreasuryDeposit` / `TreasuryWithdraw` (or a governed development
   `EmissionRole`, per O3-c) actually move value, with the supply invariant journalled
   (`invariants/balance.rs` today cannot close the supply identity).
2. **Governance proposals on chain** with a from-bound proposer (lifting the `wrfcoin/core4-archive#2024` fence), and a
   `FundingProposal` whose execution transfers (`implementation.rs:156`).
3. **An on-chain bounty record:** the spec digest at announcement, and a payout that cites the evaluator
   output digest. Each slice sits behind its own unscheduled `ConsensusFeature` with gate-off parity.

## Open questions for the owner (each with a recommended answer)

1. **Phase 1 funding.** *Recommended: an escrow wallet funded by an ordinary transfer from an existing
   owner-held test balance, published on chain, until a governed development pool exists.*
2. **Who announces bounties in Phase 1?** *Recommended: the owner, by a signed spec in the public repo.
   Governance takes over when on-chain proposals work again.*
3. **Baseline.** *Recommended: the better of persistence and climatology (what anti-gaming measures
   against), plus the trained reference model, with a margin of +0.05 mean skill.*
4. **Window and coverage.** *Recommended: 14 days, 90 % slot coverage, missing slots = 0.*
5. **Split.** *Recommended: winner-takes-all per model digest for the first bounty. Simplest to verify.*
6. **Payout timing.** *Recommended: after a 7-day dispute window, and only if the bootstrap lower bound
   of the winner's score beats the baseline plus the margin.*
