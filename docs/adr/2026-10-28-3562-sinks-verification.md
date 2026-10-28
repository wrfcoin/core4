# ADR: sinks verification, the O6-e sink slice, the 21M cap with a perpetual tail, and burns

Status: **Proposed (draft for owner ratification).** This document authorizes no change to
constants, genesis, emission, shares, burns or deployment. It writes no code. Every consensus
change it names sits behind a new `ConsensusFeature` that is not schedulable until the owner
approves it in a separate step.

Date: 2026-10-28.

Refs: `wrfcoin/core4-archive#3562` (sinks program), `wrfcoin/core4-archive#3577` (sinks via existing
verification programs), `wrfcoin/core4-archive#3561` (tokenomics), epic `wrfcoin/core4-archive#3560`.
Builds on the accepted tokenomics record `docs/adr/2026-10-23-3561-tokenomics-reconciliation.md`
(public core4 #27). That record's owner answers are settled and are not re-asked here: keep 21M,
decay with a perpetual tail, O3-c pools with no premine, burn a defined protocol inflow, burn stale
wallets, O6-e sink slice, apply on the live testnet feature-gated, and **the owner ratifies the
economics only after this design**. This document is the design that ratification waits for.

Code references are to public `core4` main `cc7f4d4`. "PoP" is
`shared-protocol/src/consensus/proof_of_publishing.rs`.

## 0. The rule every section below obeys

> "A currency that can only be earned by doing what is right for the planet."

Every sink payout must be verifiable from chain data or a signed receipt, and replay identically on
every node. A design that needs a node's local clock, its local view of the world, or one trusted
reporter has the wrong shape. Concretely, in this document:

- **Time** is chain time: block height, or the consensus block timestamp. Signed timestamps inside a
  receipt are informational, as they already are for node-service receipts (ADR-003
  role-earnings:589: "Signed timestamps remain informational; no local clock or expiring HTTP
  acknowledgement enters eligibility").
- **Randomness** (which claims are audited) comes from a block hash that did not exist when the
  claim was submitted.
- **Evidence** is a hash committed on chain. The photo, satellite tile or survey stays off chain.
  The chain pays for a signed statement about the evidence, and keeps the hash so anyone can re-check.
- **No single party** can create sink money: a claim needs a program attester's signature, and a
  sampled fraction needs a second, independent auditor's signature before it pays.

## 1. Current state (what the code does today)

| Item | Today | Source |
|---|---|---|
| Emission roles | Contribution, Attester, Producer, Node, Faucet. **No Sink.** | PoP:797 `EmissionRole` |
| Share tables | mainnet 70/15/10/5/0; testnet 69/15/10/5/1 (used by 888 through its genesis policy) | PoP:836-837; `crypto/chain_binding.rs:224-234` |
| Unclaimed slice | Mints **zero**, never redistributed, never carried into a later block | PoP:996 ("A role that claims none of its slice mints **zero**"); test `attester_slice_is_unclaimed_and_unemitted_when_nothing_settles_2846` (PoP:25772); ADR-003:253 |
| Supply cap basis | `issuance_committed_supply()` = sum of account balances + outstanding node reserves. A mint that would pass `MAX_SUPPLY` is **deferred**, not dropped | `blockchain/state.rs:1862`, `:4234` |
| Burn accounting | `supply_authorized_burning` journal (not in the state root) | `state.rs:1267`, slashing burn `:5402`, paymaster burn `:5882` |
| Signed receipt families | `ForecastVerificationReceipt` (oracle-signed forecast scores), `SignedNodeServiceReceipt` (consumer-signed service, domain tag `wrfcoin.node_service_receipt.v1`) | `forecast-score/src/blockchain.rs:296`; `consensus/node_service_receipt.rs:42,81` |
| Sink registry, claims, attesters | None | (grep `sink` / `litter` / `planting` in core4: no consensus hit) |

The fact that matters most for §5 is the cap basis: **the cap is measured against what exists now,
not against everything ever minted**, so a burn already reopens headroom under `MAX_SUPPLY` today.

## 2. Which programs, what evidence exists, who attests (#3577)

The issue asks to pay for outcomes that existing programs already verify, rather than building
verification from scratch. The survey below is a **desk survey**. Licence, API and fraud-history cells
marked *(verify)* were not confirmed against the program's current terms. A pilot needs a signed data
agreement with the program, whatever its public licence says.

| # | Program type | Example | What it verifies | Evidence format | Who attests in this design | Fit |
|---|---|---|---|---|---|---|
| P1 | Photo + GPS litter clean-up | OpenLitterMap (open-source app, GPL-3.0 code; data published under ODbL per its founder *(verify current terms)*) | A geotagged, timestamped photo with tagged litter items | Per-photo row: GPS point, timestamp, litter tags, device model | Program attester signs a batch; WRFCoin auditor re-checks a sample of photos | **Pilot candidate.** Open data makes independent audit possible. Weakness: it proves litter was *seen*, not *removed*. Pay only "picked up" records (before/after pair or a bag-weight photo), see §2.1 |
| P2 | Satellite-monitored restoration | Restor (ETH Zürich spin-out; satellite imagery incl. NICFI tropics) *(verify API/licence)* | Site boundary, land-cover change over time | Site polygon + time series of vegetation indices | Program attester signs "site alive at month N"; auditor re-checks the index from the public imagery | **Pilot candidate** for long-horizon outcomes. Pay on survival milestones (12, 24, 36 months), never on planting alone |
| P3 | Field-verified tree planting / mangroves | planting NGOs with per-tree geotags *(verify)* | Planted count, species, plot | Plot polygon, geotagged photos, survival survey | Program attester + satellite cross-check by a P2-type auditor | Later. Highest double-count risk with carbon registries (§2.3) |
| P4 | Waterway / beach clean-up events | organised clean-up days that weigh collected waste *(verify)* | Mass collected, location, date | Event record, weigh-station photo, participant count | Event organiser attester; sampled auditor review | Later. Low unit value, high Sybil pressure |
| P5 | Carbon registries (Verra, Gold Standard) | — | Tonnes CO₂ issued as credits | Registry serial numbers | — | **Do not pay in this phase.** Outcomes are already funded by credit sales (fails "not funded elsewhere"), and paying against credits invites carbon-offset marketing, which needs counsel (#3577, tokenomics ADR line 198) |

**Recommendation:** pilot with **P1 (litter, removal records only)** and **P2 (restoration survival
milestones)**. They differ in failure mode (fast, cheap, Sybil-prone vs slow, expensive, satellite-
checkable), so the pilot exercises both ends of the claim design.

### 2.1 What counts as an outcome

An **outcome** is the smallest unit a program verifies and the chain pays once:

| Program type | Outcome | Quantity unit | Paid when |
|---|---|---|---|
| Litter (P1) | one removal record: before/after photo pair or a bag photo with item tags | items removed (capped per record) | once, after audit window |
| Restoration (P2) | one site-milestone: site S alive at month M | hectares × milestone weight | once per milestone (12/24/36 m) |
| Field planting (P3) | one plot-milestone | surviving trees | once per milestone |
| Event clean-up (P4) | one weighed event | kg | once |

### 2.2 Outcome record (the on-chain claim body)

Fields, in canonical-body order (same `BodyBuilder` style as `NodeServiceReceipt::canonical_body`):

| Field | Type | Notes |
|---|---|---|
| `program_id` | string | registry key (§3.1) |
| `outcome_id` | 32 bytes | **global identity**, see §2.3 |
| `outcome_kind` | enum | `litter_removal`, `site_milestone`, `plot_milestone`, `weighed_event` |
| `cell` | geo-cell id | the same ≈2° grid that pays scarcity (`data_desert.rs`), so caps and scarcity share one grid |
| `period_start_height`, `period_end_height` | u64 | chain heights bounding when the work happened, as stated by the program |
| `quantity` | u64 | in the unit of `outcome_kind`, after the program's own verification |
| `evidence_hash` | 32 bytes | hash of the evidence bundle the program holds (photo set, tile ids + indices) |
| `beneficiary` | address | who is paid (the volunteer, the site operator, or the program's pool address) |
| `funded_elsewhere` | enum | `none`, `additive_disclosed(ref)`; any other value is refused |
| `nonce` | 32 bytes | attester uniqueness |

Domain tag: `wrfcoin.sink_outcome_attestation.v1`. Signed by one registered program attester key.

### 2.3 Double counting: one global outcome identity

`outcome_id = H("wrfcoin.sink_outcome_id.v1" ‖ outcome_kind ‖ cell ‖ physical_key)` where
`physical_key` is the thing in the world, not the program's record number:

- litter: the perceptual hash of the "after" photo, plus a rounded GPS point (≈10 m);
- restoration/planting: the site polygon hash plus the milestone month;
- events: the event polygon hash plus the event date as a chain height bucket.

Rules (consensus):

1. A claimed `outcome_id` is stored in a terminal-marker set, exactly like the attester decision
   terminal marker (ADR-003:243-247). A second claim with the same id, **from any program**, is
   refused.
2. Overlapping restoration polygons are refused when the overlap exceeds a governed fraction
   (recommended 10%) of the smaller polygon. This check needs the polygon on chain only as a bounding
   box plus hash; the exact overlap is an auditor duty (§3.3), and a confirmed overlap claws back.
3. `funded_elsewhere` other than `none` or a disclosed additive reference is refused.
4. Tests required: same photo through two programs; same site under two milestones (allowed: the
   milestone month is part of the id); same site twice under one milestone (refused); overlapping
   polygons across programs.

Perceptual hashes are not collision-proof against an adversary who edits photos. That is what the
sampled audit (§3.3) and the rate caps (§3.4) are for; the id rule stops honest double-claims and
cheap replays, not a determined forger.

## 3. How a sink is verified on chain

### 3.1 Registry (governance state)

A **sink program** enters a registry through a governance proposal. A registry row holds:
`program_id`, outcome kinds allowed, the **rate card** (WRF per unit per kind, §4.3), per-cell and
per-epoch caps, the program's attester public keys, and a bond reference (node-bond machinery,
`node_bond_registry`, generalised). A program can be paused or removed by governance; removal stops
new claims and leaves claims already in their audit window to finish or be clawed back.

Separately, governance keeps an **auditor set**: keys that do not belong to any program they audit.
The paper's "proof of time, location, labor" is this split: the program attests the labour, the
auditor re-checks location and time against independent evidence.

### 3.2 Claim flow

```
program attester signs outcome record  ──►  SinkClaim tx (any account may relay it)
   │
   ▼  admission: program active, key registered, outcome_id unseen, caps not exceeded,
   │             period ends before the claim height, funded_elsewhere valid
   ▼
claim recorded at height H (pending, NOT paid)
   │
   ├─ audit selection: seed = block hash at H + K (K = 2 blocks, so the claimant cannot know it)
   │     selected if  H(seed ‖ outcome_id) mod 10_000 < audit_bps   (recommended 500 = 5%)
   │     a selected claim gets an auditor from the auditor set, chosen by the same seed
   │
   ├─ audit window: W blocks after H (recommended: one node-share epoch, 512 blocks)
   │     selected claim: needs a signed auditor PASS receipt inside W, else it is NOT paid
   │     any claim: an auditor FAIL receipt inside W refuses it and starts a clawback (§3.5)
   │
   ▼
matured at H + W  ──►  settled from the sink slice of the epoch it matures in (§4)
```

- Every input is a signed receipt or chain data. The seed is a block hash; the window is in blocks;
  the program's own dates are heights.
- **Fail closed:** a sampled claim with no auditor receipt is not paid. A missing auditor never turns
  into a payment.
- The auditor receipt is a new signed type in the same family:
  `wrfcoin.sink_audit_receipt.v1` over `(outcome_id, verdict, reason_code, evidence_hash_checked,
  auditor_pubkey, claim_height, nonce)`. Auditors are paid a fixed fraction of each claim they audit
  (recommended 10% of that claim's payout, from the same slice), PASS or FAIL alike, so the auditor
  has no reason to favour either verdict.

### 3.3 Spot audits

| Parameter | Recommended | Reason |
|---|---|---|
| `audit_bps` | 500 (5%) | Expected loss for a cheat = 5% × (bond + clawback). With the cap in §3.4, a program faking 20% of records is caught within ~10 epochs with high probability (1 − 0.95^(n)) |
| Escalation | A FAIL raises the program's `audit_bps` to 2,500 for 4 epochs; 3 FAILs in 8 epochs auto-pause the program pending governance | Cheaters face rising audit pressure; honest programs pay little |
| Auditor independence | Auditor keys must not be registered to any program, and an auditor is never selected for a claim whose `beneficiary` or program shares its owning account | Same account-separation rule as bonded node slice 3 (ADR-003:589) |

### 3.4 Rate caps

- Per program per epoch: governed, recommended 10% of the epoch's sink budget.
- Per cell per epoch, across all programs: recommended 2% of the epoch's sink budget. Small cells
  cannot drain the slice. This is the Sybil cap for P1/P4.
- Per beneficiary per epoch: recommended 0.5% of the epoch's sink budget.
- A claim that would exceed a cap is refused at admission (not deferred), so the queue cannot grow
  without bound.

### 3.5 Clawback

Claims pay at maturity, after their audit window, so most clawback is "do not pay". After payment, a
later FAIL (governed late-audit window, recommended 4 epochs) debits the **program bond**, not the
volunteer, and burns the debited amount (it is a protocol inflow, §6.1). Programs carry the bond
because the program chose to attest the record.

## 4. The O6-e sink slice

### 4.1 Where it lives

A sixth role, `EmissionRole::Sink`, **appended after `Faucet`** in `EMISSION_ROLE_ORDER`. Appending
keeps the largest-remainder tie-break order of the five existing roles (PoP:819). The shares come from
contribution, as the owner's O6-e example does:

| Table | Contribution | Attester | Producer | Node | Faucet | **Sink** |
|---|---|---|---|---|---|---|
| mainnet today | 70% | 15% | 10% | 5% | 0% | — |
| mainnet O6-e | **65%** | 15% | 10% | 5% | 0% | **5%** |
| testnet today (888) | 69% | 15% | 10% | 5% | 1% | — |
| testnet O6-e | **64%** | 15% | 10% | 5% | 1% | **5%** |

Shares still sum to exactly 10,000 bps, so the remainder bound becomes `EMISSION_ROLE_COUNT − 1 = 5`.
`EMISSION_ROLE_COUNT` changes from 5 to 6, which is wire- and root-affecting (tokenomics ADR §6) and
is why the table is selected by a feature at a height (§7), never edited in place.

On 888 at today's measured cadence (77 s, cadence report §1), 5% of the 22.22 WRF ceiling is
**1.111 WRF per block, about 1,247 WRF/day ceiling**. At the 600 s design basis it is 160 WRF/day.

### 4.2 Zero-claim epochs mint nothing

The sink slice follows the attester rule exactly:

- The **budget** is per block: `slice_sink(h) = partition(h).slice(Sink)`.
- Claims that mature in block `h` divide that block's slice. The **epoch** (§3.2's 512-block window)
  is only the unit for caps; budget is not pooled across an epoch.
- If no claim matures in a block, the slice is unclaimed and mints **zero**. It is not carried into a
  later block, not handed to another role, and not accrued anywhere (PoP:996).
- If matured claims exceed the slice, the excess stays pending and retries in the next block in
  `(maturity height, outcome_id)` order, the same deferral shape as the `MAX_SUPPLY` path
  (`state.rs:4234`). This defers *claims*, not *budget*: a later block still mints at most its own
  slice. Because §3.4 refuses over-cap claims at admission, the pending queue is bounded.

Required test: a run of blocks with no matured claims mints exactly zero to `Sink`, and the following
block with a claim mints at most one block's slice (no rollover).

### 4.3 How much each claim gets

`payout = min(rate_card(kind) × quantity, remaining slice in the block)`, with the auditor's fraction
taken from the payout. A rate card bounds the per-unit price, so a single tiny claim in an otherwise
empty block cannot take the whole slice. Recommended starting rates are an owner question (Q4); this
document does not set a price.

### 4.4 Growth by governance

A governed parameter `sink_share_bps`, starting at 500, with:
- a step limit of +250 bps per change, and at least 4 node-share epochs between changes;
- a ceiling of 1,500 bps (the paper's "at least 15%") until a further ratification;
- the extra bps taken from contribution only, so no other role's economics move.

Changing a share is a monetary change, so every change takes effect at a future height through the
same activation machinery as any feature, never mid-block.

## 5. Reconciling the 21M cap with a perpetual tail

The owner chose both "keep 21M" and "decay with a perpetual tail". They are compatible under one
reading only, and the code already uses it:

> **21M caps circulating supply** (what exists now: balances plus node reserves), not cumulative
> issuance.

Under a cap on cumulative issuance, a perpetual tail is impossible unless the tail goes to zero.
Under a cap on circulating supply, a tail is funded by what is burned. `issuance_committed_supply()`
(`state.rs:1862`) already sums balances, so burned value already frees headroom.

Both options below keep circulating supply ≤ 21M forever. Model: mainnet default curve (period-1 rate
7,000,000 WRF per 126,000,000 s = **1,753,200 WRF/yr**). Burns are an **assumption** of 0.25% of
circulating supply per year, used only to make the tail visible; the real figure depends on the
inflow defined in §6.1. Script: integration at 0.01-year steps.

### Option A: today's ⅔-step curve, plus a recycled tail

Issuance per year = `max(scheduled curve, burned)`, bounded by headroom. While the curve exceeds
burns, nothing changes from ADR-002. Once the curve drops below burns, the chain re-mints exactly what
was burned.

### Option B: asymptotic to the cap (headroom-proportional)

Issuance per year = `h × (21M − circulating)`, with `h = 1,753,200 / 21,000,000 = 8.35%` per year
(chosen so year-1 matches today's period-1 rate). Issuance never reaches zero while there is
headroom, and every burn raises headroom, so the tail is both asymptotic and burn-funded with one
formula.

| Year | A issued/yr | A cumulative minted | A cumulative burned | A circulating | B issued/yr | B cumulative minted | B cumulative burned | B circulating |
|---|---|---|---|---|---|---|---|---|
| 1 | 1,753,200 | 1,753,200 | 2,168 | 1,751,032 | 1,614,239 | 1,682,743 | 2,110 | 1,680,633 |
| 2 | 1,753,200 | 3,506,400 | 8,708 | 3,497,692 | 1,485,387 | 3,230,989 | 8,247 | 3,222,741 |
| 4 | 1,168,800 | 7,006,956 | 34,861 | 6,972,095 | 1,258,670 | 5,967,415 | 31,305 | 5,936,110 |
| 8 | 779,200 | 11,674,364 | 127,475 | 11,546,889 | 907,075 | 10,256,901 | 112,857 | 10,144,043 |
| 12 | 519,467 | 14,783,372 | 257,864 | 14,525,508 | 657,839 | 13,356,952 | 230,029 | 13,126,923 |
| 16 | 346,311 | 16,856,044 | 412,726 | 16,443,318 | 481,163 | 15,613,847 | 372,449 | 15,241,398 |
| 20 | 230,874 | 18,236,671 | 583,227 | 17,653,444 | 355,922 | 17,273,055 | 532,767 | 16,740,287 |
| 30 | 102,611 | 19,974,127 | 1,043,922 | 18,930,205 | 179,990 | 19,827,153 | 981,296 | 18,845,857 |
| 40 | 47,800 | 20,640,680 | 1,520,844 | 19,119,836 | 105,558 | 21,201,814 | 1,465,143 | 19,736,671 |
| 50 | 47,800 | 21,118,676 | 1,998,840 | 19,119,836 | 74,068 | 22,077,485 | 1,963,933 | 20,113,551 |
| 100 | 47,800 | 23,508,656 | 4,388,819 | 19,119,836 | 51,288 | 24,890,930 | 4,504,748 | 20,386,182 |
| 200 | 47,800 | 28,288,615 | 9,168,778 | 19,119,836 | 50,975 | 29,992,049 | 9,602,121 | 20,389,928 |

Reading the table:

- Both options keep **circulating supply under 21M at every year**. Cumulative minted passes 21M in
  both (around year 48 in A, year 38 in B). That is what "perpetual tail" means: past issuance minus
  burns.
- **A** has a kink. Between years 30 and 40 the ⅔-step curve falls below burns and issuance switches to a
  flat "re-mint the burn" mode, so circulating supply freezes where the curve ran out (19.12M under
  this assumption). With no burns at all, A's tail is zero: the integer curve floors to nothing after
  about 60 periods (~240 years), and the "perpetual" tail is only as large as the burns.
- **B** is one formula with no switch. It is slightly less front-loaded (year 1: 1.61M vs 1.75M;
  year 4: 5.94M vs 6.97M circulating), and it keeps a tail even with zero burns (vanishing, but
  never zero until integer flooring at ~250 years). With burns, its tail settles at the burn rate.
- With 888's flat reward (22.22 WRF/block), B is simply the flat reward capped by
  `h × headroom / blocks_per_year`. At 888's current supply the headroom term is far above the flat
  reward, so 888 behaviour would not change for decades of testnet time.

**Recommendation: Option B (asymptotic, headroom-proportional).** It satisfies both owner answers
with one deterministic formula (`h`, `MAX_SUPPLY` and the state's circulating supply, already
computed as `cached_supply` for node settlement). It has no switch-over height to argue about, and it
makes the burn-funded tail automatic rather than a second mechanism. Cost: it replaces the ADR-002
curve and its pinned tests (`the_emission_parameters_satisfy_the_21m_constraint_2492`,
`the_reward_sequence_is_exactly_the_floored_two_thirds_chain_2492`). Those must be replaced on
purpose, under the new feature, never "fixed" to pass.

Determinism note for B: the per-block reward must read circulating supply **at the start of the
block** (the parent state), never mid-block, so producer, applier and WAL replay compute the same
`E(t)`. The supply journal is not in the state root (`state.rs:1254-1259`), so B must read the
committed balance sum (`checked_total_minted_supply`), not the journal.

## 6. Burns

### 6.1 The burned "protocol inflow"

Defined as value that reaches the protocol without belonging to anyone, all on chain:

| Inflow | Today | Proposal |
|---|---|---|
| Slashing | Burned (`state.rs:5402`) | unchanged |
| Paymaster sponsored spend | Burned (`state.rs:5882`) | unchanged |
| Node-share fraud forfeiture | Moved to a non-spendable protocol account, still counted as supply (tokenomics ADR §1.4) | **Burn** at forfeiture |
| Sink clawback from program bonds (§3.5) | — | **Burn** |
| Transaction fees | Producer keeps all | **Burn 50%** of each fee (a base-fee burn); the producer keeps 50% |
| Future private-analysis / compute fees (paper §5.3/5.5; `COMPUTATIONAL_FEE_ACTIVATION_HEIGHT`, PoP:167) | No split | **Burn 50%, sink slice 50%** when that fee exists (the paper funds sinks from these fees) |
| Stale-wallet balances (§6.2) | — | **Burn** |

The 50% fee burn is the recommended value: enough that heavy chain use funds the tail, while
producers still earn on fees. A fee burn changes producer economics and is root-affecting.

### 6.2 Stale wallets

**Activity:** any transaction **signed by the account's key** and included in a block (a send, a
stake action, a governance vote, or a zero-value `KeepAlive` transaction). Receiving funds is
**not** activity: if it were, anyone could keep a lost wallet alive with dust, and the rule would
never fire.

**Inactivity period:** **10 years of chain time** (the paper's figure, §5.6), measured with the
consensus block timestamp of the last signed activity against the current block's timestamp. Block
timestamps are consensus data, so this replays identically. Heights are the wrong unit here, because
the block interval is governed (story 4) and would change the period.

**Notice and sweep:**
- At 9 years the account is marked `dormant` in state (visible on chain and in wallets). Any signed
  transaction clears it.
- At 10 years a deterministic sweep burns the balance: at most N accounts per block (recommended 64),
  in address order, so cost is bounded and every node sweeps the same accounts in the same block.
- **Exempt:** protocol accounts (the faucet sink, the node-forfeiture account, any `EmissionRole`
  pool), accounts with locked principal (bonds, stake, escrow, pending withdrawals), contract and
  smart-account code accounts, and the program bonds of §3.1.

**First possible burn:** tracking starts when the activity feature activates. No account's clock can
start before then, so **the earliest burn is activation + 10 years**. There is no retroactive burn
and nothing on 888 is affected inside any realistic testnet lifetime.

This rule is irreversible for the account it burns. It needs public notice in wallet copy long before
the first burn, and the `dormant` flag exists to make that notice visible on chain.

## 7. Feature-gate plan (no code in this ADR)

Each is a new `ConsensusFeature` appended at the **end** of the enum and of `KNOWN_FEATURES`, with
`schedulable()` returning `false` until the owner approves it. Each needs gate-off parity (same
`state_root` as main with the feature off) and producer / applier / WAL parity
(`create_next_block`, `extract_transitions`, `load_wal_transitions_range`) with it on.

| # | Feature (suggested name) | What it switches on | Root-affecting | Depends on |
|---|---|---|---|---|
| F1 | `sink_registry` | registry rows, program and auditor key sets, bonds; governance proposal kinds | yes (new state) | — |
| F2 | `sink_emission_slice` | 6-role partition with the O6-e tables (§4.1), `EMISSION_ROLE_COUNT` = 6, Sink appended last | yes | F1 |
| F3 | `sink_claim_settlement` | `SinkClaim` tx, outcome-id terminal markers, audit selection, auditor receipts, maturity payout, clawback | yes | F1, F2 |
| F4 | `sink_share_governance` | governed `sink_share_bps` with the step limits of §4.4 | yes | F2 |
| F5 | `protocol_inflow_burn` | fee burn split; forfeiture and clawback burn | yes | — |
| F6 | `account_activity_tracking` | per-account last-signed-activity timestamp; `KeepAlive` tx | yes | — |
| F7 | `stale_wallet_burn` | dormant flag at 9 y, sweep at 10 y | yes | F6, activated ≥ 10 y of F6 history to have any effect |
| F8 | `emission_headroom_tail` | Option B emission (§5) | yes | owner ratification of §5 |

F6 should ship well before F7. It is cheap, harmless on its own, and it starts the clock.

Follow-ups the issues require, to be filed after ratification (not filed by this draft):
`wrfcoin/backend` (sink registry / claim API, program attester relay) and `wrfcoin/mobile` (submit
proof of clean-up through a registered program, view sinks), per #3562's acceptance criteria.

## 8. Risks

- **Verification quality varies by program.** Mitigated by per-program rate cards, caps and audit
  escalation, not by trusting the program.
- **Sybil "clean-ups".** Per-cell and per-beneficiary caps (§3.4), with removal records only.
- **Programs' terms may forbid reuse.** Each pilot needs a signed data agreement before its registry
  row is proposed.
- **Carbon-offset marketing.** No sink payout is described or marketed as an offset or credit
  without counsel (#3577). Testnet tokens carry no promised value (`GENESIS_888.md`). Sink rewards
  are never marketed as money.
- **Burns are irreversible.** The stale-wallet burn especially (§6.2).

## 9. Questions for the owner (each with a recommended answer)

1. **Cap reading:** is 21M a cap on circulating supply (balances + reserves) or on cumulative
   issuance? *Recommended: circulating.* That is how `state.rs:1862` already measures it, and it is
   the only reading under which a perpetual tail exists.
2. **Tail shape:** Option A (⅔ curve + recycled tail) or Option B (asymptotic, `h` = 8.35%/yr of
   headroom)? *Recommended: B.*
3. **Pilot programs:** P1 litter-removal records and P2 restoration survival milestones?
   *Recommended: yes, both, each behind a signed data agreement.* No carbon-registry outcomes (P5)
   in this phase.
4. **Rate cards:** who proposes the first WRF-per-unit prices? *Recommended: the LEAD drafts them
   from the 888 sink budget (≈1,247 WRF/day ceiling at 77 s) so that each pilot's per-epoch cap is
   10% of budget; the owner approves the governance proposal.*
5. **Audit rate and window:** 5% sampled audit, 512-block audit window, escalation to 25% after a
   FAIL? *Recommended: yes.*
6. **Auditor pay:** 10% of each audited claim, PASS or FAIL alike? *Recommended: yes.*
7. **Starting slice:** 5% from contribution (65/15/10/5/0/5 mainnet, 64/15/10/5/1/5 testnet)?
   *Recommended: yes, the owner's own O6-e example.*
8. **Growth limits:** +250 bps per change, ≥ 4 epochs apart, ceiling 1,500 bps until re-ratified?
   *Recommended: yes.*
9. **Fee burn:** 50% of transaction fees burned, producer keeps 50%? *Recommended: yes.*
10. **Forfeiture:** burn node-share fraud forfeitures instead of holding them in a protocol account?
    *Recommended: yes.*
11. **Stale wallets:** activity = any signed transaction (not receipts); period = 10 years of
    consensus block time; dormant flag at 9 years; exemptions as §6.2? *Recommended: yes.*
12. **Order of activation on 888:** F6 first (starts the clock, no economic effect), then F1–F3
    together, then F5; F8 and F4 only after a sinks pilot has run? *Recommended: yes.*
